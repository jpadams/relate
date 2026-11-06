(ns jepsen.relate.compare.diff
  "The differential engine: execute one statement on both engines,
  canonicalize and compare the results, and shrink a divergence to a
  minimal Cypher case.

  Comparison contract:

  - ROWS ARE MULTISETS. Without ORDER BY, row order is unspecified by
    Cypher, so both engines' rows are compared as frequency maps over a
    canonical rendering.

  - NUMBERS COMPARE NUMERICALLY. Cypher's `=` says 1 = 1.0, so every
    finite number canonicalizes through ONE path (canonical-number): an
    exact BigDecimal with trailing zeros stripped, so an integral double
    and its integer form land on the same key at any magnitude.
    Non-integral doubles are exact (the generator only emits exact
    binary fractions and never accumulates them — see the gen ns
    docstring), so every numeric mismatch is a real divergence.

  - ERRORS: an op that succeeds on one engine and fails on the other is
    a divergence. An op that fails on BOTH is not — error text is not
    part of the shared floor contract — but the workload records and
    reports every both-error as a generator-coverage observation.

  - UNCOMPARABLE VALUES (nodes, relationships, paths returned bare)
    throw: the generator is restricted to scalar-comparable projections,
    and a violation is a harness bug to fix, not a comparison to guess
    at.

  Shrinking is greedy over jepsen.relate.compare.shrink/op-candidates:
  every candidate is strictly smaller by shrink/weight, each candidate is
  re-executed against BOTH engines from a wiped, rebuilt graph, and a
  candidate is adopted only if the divergence survives. The loop
  terminates because weight strictly decreases."
  (:require [jepsen.relate [client :as c]]
            [jepsen.relate.compare [render :as render]
                                       [shrink :as shrink]])
  (:import (java.math BigDecimal)
           (org.neo4j.driver Driver Record Value)
           (org.neo4j.driver.exceptions Neo4jException)))

;;;; Canonicalization

(defn- canonical-number
  "The one canonical form of a finite number: an exact BigDecimal with
  trailing zeros stripped. Longs convert exactly; doubles go through
  BigDecimal/valueOf (the shortest decimal that round-trips the double),
  so 42 and 42.0 both become 42M and 1.0E16 and 10000000000000000 both
  become 1E+16M — numerically-equal values share one key at every
  magnitude, with no cutoff."
  ^BigDecimal [v]
  (.stripTrailingZeros
   ^BigDecimal (if (integer? v)
                 (bigdec (long v))
                 (BigDecimal/valueOf (double v)))))

(defn canonical
  "A Java driver value as engine-neutral comparable data. Maps are
  sorted (property-map key order is an implementation detail), lists
  recurse, finite numbers go through canonical-number (integral doubles
  unify with their integer form)."
  [v]
  (cond
    (nil? v)        [:null]
    (instance? Boolean v) [:bool v]
    (string? v)     [:str v]
    (integer? v)    [:num (canonical-number v)]
    (float? v)      (cond
                      (Double/isNaN v)     [:num :nan]
                      (Double/isInfinite v) [:num (if (pos? v) :pos-inf :neg-inf)]
                      :else [:num (canonical-number v)])
    (instance? java.util.List v) [:list (mapv canonical v)]
    (instance? java.util.Map v)
    [:map (into (sorted-map)
                (map (fn [[k v]] [(canonical k) (canonical v)]))
                v)]
    :else
    (throw (ex-info (str "uncomparable value of type "
                         (.getName (class v))
                         " — the generator must project to scalars")
                    {:value (pr-str v)}))))

(defn- row-freqs
  "Rows as a frequency map keyed on their pr-str rendering — sound
  because canonical already reduced every number to its one stripped
  BigDecimal form, so numerically-equal values render identically."
  [rows]
  (frequencies (map pr-str rows)))

(defn divergent?
  "Do two exec! outcomes disagree? Outcome kind mismatch (one ok, one
  error) or row-multiset mismatch."
  [a b]
  (or (not= (:outcome a) (:outcome b))
      (and (= :ok (:outcome a))
           (not= (row-freqs (:rows a)) (row-freqs (:rows b))))))


;;;; Execution

(defn exec!
  "Runs `stmt` with `params` on `driver` in one auto-commit unit.
  Returns {:outcome :ok, :rows [canonical rows]} or {:outcome :error,
  :msg}. A received FAILURE (a Neo4jException that client's
  connection-error? does not classify as connection-level) is an
  outcome, not an exception; connection-level errors — including the
  Neo4jException subtypes ServiceUnavailableException and
  SessionExpiredException — propagate: with no nemesis in this lane they
  are an infrastructure fault for jepsen to report as :info, never a
  divergence or a tolerated both-error."
  [^Driver driver stmt params]
  (try
    (with-open [session (.session driver)]
      (let [^org.neo4j.driver.Result result
            (.run session stmt (c/bolt-params params))
            records (.list result)
            rows (mapv (fn [^Record rec]
                         (mapv (fn [^Value v] (canonical (.asObject v)))
                               (.values rec)))
                       records)]
        {:outcome :ok :rows rows}))
    (catch Neo4jException e
      (if (c/connection-error? e)
        (throw e)
        {:outcome :error :msg (.getMessage e)}))))

;;;; State reads (mutation comparison)

(defn wipe!
  "Empties the graph. Throws unless the wipe succeeded — a failed wipe
  would silently compare against stale state."
  [driver]
  (let [r (exec! driver "MATCH (n) DETACH DELETE n" {})]
    (when-not (= :ok (:outcome r))
      (throw (ex-info (str "wipe failed: " (:msg r)) r)))))

(defn state-read
  "The full canonical state: every node and relationship, keyed by
  user-visible identity, deterministic order. Two engines agree on a
  mutation iff their state reads agree exactly."
  [driver]
  (let [nodes (exec! driver render/state-read-nodes {})
        rels  (exec! driver render/state-read-rels {})]
    (if (and (= :ok (:outcome nodes)) (= :ok (:outcome rels)))
      {:outcome :ok :rows (concat (:rows nodes) (:rows rels))}
      {:outcome :error
       :msg (str "state read failed: "
                 (or (:msg nodes) "") " / " (or (:msg rels) ""))})))

;;;; Case reproduction

(defn run-case!
  "Rebuilds `case` from scratch on both engines — wipe, replay setup,
  run the query — and reports whether the divergence reproduces.
  :verify :rows compares the query's own rows; :verify :state compares
  the canonical state read after the query (for mutations)."
  [cl-driver neo-driver {:keys [setup query verify] :as _case}]
  (wipe! cl-driver)
  (wipe! neo-driver)
  (doseq [{:keys [stmt]} setup]
    (exec! cl-driver stmt {})
    (exec! neo-driver stmt {}))
  (let [qa (if query (exec! cl-driver (:stmt query) (:params query))
               {:outcome :ok :rows []})
        qb (if query (exec! neo-driver (:stmt query) (:params query))
               {:outcome :ok :rows []})]
    (if (= :rows verify)
      {:divergent? (divergent? qa qb) :cl qa :neo qb}
      (let [sa (state-read cl-driver)
            sb (state-read neo-driver)]
        (if (divergent? qa qb)
          {:divergent? true :cl qa :neo qb}
          {:divergent? (divergent? sa sb) :cl sa :neo sb})))))

(defn restore!
  "Rebuilds both engines to the same mutated state after a divergence:
  wipe, then replay the (symmetric) statement history in order. The
  divergent statement itself is never in this history — the client
  excludes it, so both engines end identical and later ops compare
  against symmetric state."
  [cl-driver neo-driver history]
  (wipe! cl-driver)
  (wipe! neo-driver)
  (doseq [{:keys [stmt]} history]
    (exec! cl-driver stmt {})
    (exec! neo-driver stmt {})))

;;;; Shrinking

(defn- case-weight [case]
  (shrink/weight {:setup (mapv :data (:setup case))
                :qdata (:qdata case)
                :params (get-in case [:query :params])}))

(defn- case-candidates
  "All smaller versions of a case, cheapest-first: try the empty graph
  outright (many floor divergences need no data at all — one candidate
  instead of a statement-by-statement walk), then drop or simplify one
  setup statement, then simplify the op data, then simplify one
  parameter value."
  [{:keys [setup qdata] :as case}]
  (let [n (count setup)]
    (concat
      ;; the empty graph, tried outright
      (when (pos? n) [(assoc case :setup [])])
      ;; drop a setup statement
      (for [i (range n)] (update case :setup shrink/drop-nth i))
      ;; simplify one setup statement's data
      (for [i (range n)
            c (shrink/op-candidates (:data (nth setup i)))]
        (update-in case [:setup i] assoc :data c
                   :stmt (render/render-stmt c)))
      ;; simplify the op data
      (for [c (shrink/op-candidates qdata)]
        (-> case
            (assoc :qdata c)
            (assoc-in [:query :stmt] (render/render-stmt c))))
      ;; simplify one parameter value
      (for [c (shrink/lit-variants (get-in case [:query :params]))]
        (assoc-in case [:query :params] c)))))

(defn shrink-case!
  "Greedy shrink to a minimal reproducing case: adopt the first strictly
  smaller candidate that still diverges, repeat until a full candidate
  pass adopts nothing. Terminates because adopted weight strictly
  decreases."
  [cl-driver neo-driver case]
  (loop [case case]
    (let [w (case-weight case)
          smaller (filter #(and (< (case-weight %) w)
                                (:divergent? (run-case! cl-driver neo-driver %)))
                          (case-candidates case))]
      (if (seq smaller)
        (recur (first smaller))
        case))))
