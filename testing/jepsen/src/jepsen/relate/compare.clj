(ns jepsen.relate.compare
  "The compare workload (LABS-657): a random graph and op generator over
  the shared openCypher floor, executed against BOTH the Relate DB
  node and a real-Neo4j reference container, results compared row by
  row, every divergence shrunk to a minimal Cypher case.

  This is the cheapest-bug-finder lane, not a Jepsen fault-injection
  lane: no nemesis (a fault here would only manufacture spurious
  divergences), one client, strictly sequential ops — the comparison is
  the point. It runs ON DEMAND (./run.sh compare), never per-PR.

  On a divergence the client shrinks the case (diff/shrink-case!),
  records the minimal case on the op and in the log, resynchronizes both
  engines from the statement history (the divergent statement excluded),
  and keeps going — one run surfaces every divergence it can reach. The
  checker fails the run on any divergence, spits the full list as JSON
  to divergences.json in the run's store directory, and reports
  both-error statements (a generator-coverage observation, not a
  divergence: error text is not part of the floor contract)."
  (:require [clojure.data.json :as json]
            [clojure.tools.logging :refer [info warn]]
            [jepsen [checker :as checker]
                    [client :as client]
                    [db :as db]
                    [generator :as gen]
                    [nemesis :as nemesis]
                    [store :as store]
                    [tests :as tests]]
            [jepsen.relate [client :as c]
                               [db :as cdb]
                               [docker :as docker]
                               [util :as util]]
            [jepsen.relate.compare [diff :as diff]
                                       [gen :as cgen]]))

(def ^:const neo4j-boot-timeout-secs
  "How long setup waits for the reference container to accept Bolt
  connections — Neo4j's own boot (plugin scanning, recovery) is the
  slowest thing in this lane."
  180)

;;;; DB lifecycle: Relate under test + the Neo4j reference

(defn- neo4j-wipe!
  "Wipes the reference graph over Bolt. The reference container has no
  persistent volume (recreating it is the compose-level wipe), so this
  wipe is what makes a rerun against a still-running container start
  from empty."
  [opts]
  (let [driver (c/driver* (:neo4j-uri opts)
                          (:neo4j-user opts)
                          (:neo4j-password opts))]
    (try
      (diff/wipe! driver)
      (info "neo4j reference wiped")
      (finally (util/close-quietly! driver)))))

(defn compare-db
  "A jepsen.db/DB for the compare lane: the set lane's DB lifecycle
  (kill, wipe volume, restart, gate on the pinned identity) plus the
  reference's — wait for Neo4j to accept Bolt, then wipe it so both
  engines start empty. The bolt surface's :impl is mandatory — the DB
  gate verifies recovery on the lane's own wire, never a fallback."
  [opts]
  (let [inner (cdb/docker-db (assoc opts
                                    :store :volume
                                    :uri (:bolt-uri opts)
                                    :impl (cdb/surface-impl :bolt)))]
    (reify db/DB
      (setup! [this test node]
        (db/setup! inner test node)
        (let [agent (util/retry-until
                     #(c/probe-unchecked! (:neo4j-uri opts)
                                          (:neo4j-user opts)
                                          (:neo4j-password opts))
                     util/any neo4j-boot-timeout-secs 500)]
          (info "neo4j reference up:" agent)
          (neo4j-wipe! opts)))

      (teardown! [this test node]
        (db/teardown! inner test node)
        (spit (store/path! test "neo4j-container.log")
              (str (docker/logs-record (:neo4j-container opts)) "\n"))))))

;;;; The client

(declare resync! diverge! apply-mutation! compare-read! final-read!)

(defrecord CompareClient [cl-driver neo-driver history]
  client/Client
  (open! [this test _node]
    ;; Atomic: if the identity probe or the reference driver's
    ;; construction throws, the already-built Relate driver is
    ;; closed before the failure propagates — a failed open! leaks
    ;; nothing.
    (let [opts (:compare-opts test)
          cl (c/driver* (:bolt-uri opts) (:auth-user opts)
                        (:auth-password opts))]
      (try
        (c/assert-identity! (c/server-agent cl))
        (let [neo (c/driver* (:neo4j-uri opts)
                             (:neo4j-user opts)
                             (:neo4j-password opts))]
          (assoc this :cl-driver cl :neo-driver neo :history (atom [])))
        (catch Throwable t
          (util/close-quietly! cl)
          (throw t)))))

  (setup! [this _test])

  (invoke! [this test op]
    (case (:f op)
      :create     (apply-mutation! this op (:value op))
      :write      (apply-mutation! this op (:value op))
      :read       (compare-read! this op (:value op))
      :final-read (final-read! this op)))

  (teardown! [this _test])

  (close! [this _test]
    (util/close-quietly! cl-driver)
    (util/close-quietly! neo-driver))

  client/Reusable
  (reusable? [this _test] true))

(defn- resync!
  "After a divergence: rebuild both engines from the symmetric history
  so subsequent ops compare like with like."
  [{:keys [cl-driver neo-driver history]}]
  (diff/restore! cl-driver neo-driver @history))

(defn- diverge!
  "Record a divergence: build the case — the symmetric statement history
  as its setup (read from the client's own history, so no call site
  restates it), the op data, the query, and how to verify it — shrink it
  to the minimal reproducing case, re-run it once so the recorded case
  carries both engines' actual outcomes, log it loudly, resync both
  engines, fail the op carrying the minimal case."
  [{:keys [cl-driver neo-driver history] :as this} op qdata query verify]
  (let [case {:setup @history :qdata qdata :query query :verify verify}
        minimal (diff/shrink-case! cl-driver neo-driver case)
        final (diff/run-case! cl-driver neo-driver minimal)
        ;; the recorded case carries its own outcomes, so the log, the
        ;; op, and divergences.json each stand alone as a finding
        minimal (assoc minimal
                       :relate (select-keys (:cl final)
                                                [:outcome :rows :msg])
                       :neo4j (select-keys (:neo final)
                                           [:outcome :rows :msg]))]
    (warn "\nDIVERGENCE (minimal case)\n"
          "  setup:  " (pr-str (mapv :stmt (:setup minimal))) "\n"
          "  query:  " (:stmt (:query minimal))
          "  params: " (pr-str (:params (:query minimal))) "\n"
          "  relate: " (pr-str (:relate minimal)) "\n"
          "  neo4j:      " (pr-str (:neo4j minimal)))
    (resync! this)
    (assoc op :type :fail :divergence minimal)))

(defn- apply-mutation!
  "A CREATE (setup) or generated write: run on both, then compare the
  canonical state. Both-error has two readings: a failed CREATE is a
  generator bug (CREATEs here cannot legitimately fail on the floor) and
  throws; a failed write can be legitimate (bare DELETE on a related
  node errors on both engines) and is recorded as a both-error
  observation without entering the replay history — an errored
  auto-commit statement mutated nothing. On divergence the statement is
  excluded from the replay history so the resync leaves both engines
  identical."
  [{:keys [cl-driver neo-driver history] :as this} op {:keys [stmt data]}]
  (let [a (diff/exec! cl-driver stmt {})
        b (diff/exec! neo-driver stmt {})]
    (cond
      (and (= :error (:outcome a)) (= :error (:outcome b)))
      (if (= :create (:f op))
        (throw (ex-info (str "setup statement failed on BOTH engines — "
                             "generator bug, not a divergence\n"
                             stmt "\n  relate: " (:msg a)
                             "\n  neo4j: " (:msg b))
                        {:stmt stmt :cl (:msg a) :neo (:msg b)}))
        (assoc op :type :ok :both-error (:msg a)))

      (= :ok (:outcome a) (:outcome b))
      (do (swap! history conj {:stmt stmt :data data})
          (let [sa (diff/state-read cl-driver)
                sb (diff/state-read neo-driver)]
            (if (diff/divergent? sa sb)
              (do
                ;; the divergent statement leaves asymmetric state: drop
                ;; it from history, then diverge! resyncs from the rest
                (swap! history pop)
                (diverge! this op data {:stmt stmt :params {}} :state))
              (assoc op :type :ok))))

      :else
      ;; one engine errored, the other committed: the divergence IS the
      ;; outcome mismatch. The statement never enters the history; the
      ;; resync wipes whatever the succeeding side did.
      (diverge! this op data {:stmt stmt :params {}} :state))))

(defn- compare-read!
  "A read: run on both, compare row multisets. Both-error is recorded on
  the op (the checker reports it as generator-coverage observation)."
  [{:keys [cl-driver neo-driver] :as this} op {:keys [stmt params data]}]
  (let [a (diff/exec! cl-driver stmt params)
        b (diff/exec! neo-driver stmt params)]
    (cond
      (diff/divergent? a b)
      (diverge! this op data {:stmt stmt :params params} :rows)

      (= :error (:outcome a))
      (assoc op :type :ok :both-error (:msg a))

      :else
      (assoc op :type :ok))))

(defn- final-read!
  "The closing comparison: full canonical state of both engines."
  [{:keys [cl-driver neo-driver] :as this} op]
  (let [sa (diff/state-read cl-driver)
        sb (diff/state-read neo-driver)]
    (if (diff/divergent? sa sb)
      (diverge! this op nil nil :state)
      (assoc op :type :ok))))

;;;; Checker

(defn compare-checker
  "Fails the run on any divergence or any :info op (no faults exist in
  this lane, so :info means infrastructure trouble). Writes the full
  divergence list — minimal cases — to divergences.json in the run's
  store directory (data.json — the http lane's dependency — encodes it)
  and embeds the same JSON in the check result."
  []
  (reify checker/Checker
    (check [_this test history _opts]
      (let [completions (remove #(= :invoke (:type %)) history)
            divs (mapv :divergence (filter :divergence completions))
            infos (filter #(= :info (:type %)) completions)
            both-errors (mapv (fn [op]
                                {:f (:f op)
                                 :stmt (get-in op [:value :stmt])
                                 :msg (:both-error (:value op))})
                              (filter #(get-in % [:value :both-error])
                                      completions))
            json (json/write-str divs)]
        (spit (store/path! test "divergences.json") (str json "\n"))
        {:valid? (and (empty? divs) (empty? infos))
         :divergence-count (count divs)
         :divergences divs
         :divergences-json json
         :both-error-count (count both-errors)
         :both-errors (take 10 both-errors)
         :unexpected-info-count (count infos)
         :unexpected-info-fs (mapv :f infos)}))))

;;;; Generator

(defn compare-generator
  "The finite op plan: the generated graph's CREATE statements, then
  `ops` generated read/write ops, then one final state comparison.
  Wrapped in a generous time limit so a stalled engine ends the run
  instead of hanging it."
  [opts seed]
  (let [r (cgen/rng seed)
        graph (cgen/gen-graph r)
        creates (cgen/setup-stmts graph)
        ops (cgen/gen-ops r (:node-count graph) (:ops opts))
        plan (concat
               (map (fn [s] {:type :invoke :f :create :value s}) creates)
               (map (fn [o] {:type :invoke :f (:kind o) :value o}) ops)
               [{:type :invoke :f :final-read :value {}}])]
    (info "compare workload: seed" seed
          ", graph" (count (:nodes graph)) "nodes /" (count (:rels graph))
          "rels, plan" (count plan) "ops")
    (gen/clients (gen/time-limit (:time-limit opts) plan))))

;;;; Test

(defn compare-test
  "The compare workload's test map: one node (Relate under test),
  one client (both drivers), noop nemesis, sequential ops."
  [opts]
  (let [seed (long (or (:seed opts) (System/currentTimeMillis)))]
    (merge tests/noop-test
           {:name (str "relate-compare-" seed)
            :nodes [(:db-host opts)]
            :ssh {:dummy? true}
            :db (compare-db opts)
            :client (->CompareClient nil nil nil)
            :nemesis nemesis/noop
            :checker (checker/compose {:stats (checker/stats)
                                       :compare (compare-checker)})
            :generator (compare-generator opts seed)
            ;; Comparing requires strict sequencing: one client, and no
            ;; jepsen concurrency at all.
            :concurrency 1
            :compare-opts opts})))
