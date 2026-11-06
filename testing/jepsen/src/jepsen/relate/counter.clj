(ns jepsen.relate.counter
  "The Elle counter lane (LABS-652): list-append over a small pool of
  :Counter nodes, one per key, the `log` list property as the append log.

  One Jepsen op = one explicit Bolt transaction = one Elle txn of 1-3
  micro-ops over distinct keys: reads ([:r k observed-log]) and appends
  ([:append k v]). Generator constraint, per the lane design: a
  transaction may read key k only if it also appends to k — no read-only
  transactions, no reads of keys it does not also write (final-state
  reads stay outside the Elle history; the in-txn reads are what let
  Elle infer the complete Adya version order per key from a single
  read). A read is emitted immediately before its key's append, so it
  observes the BEGIN-pinned snapshot — never the transaction's own
  writes.

  Append values are client-generated globally unique integers from ONE
  shared monotonic counter — per-key uniqueness is elle's hard
  requirement (:duplicate-appends otherwise), and the generator cannot
  know which process will claim an op, so the design's 'process tag +
  monotonic counter' collapses into the shared monotonic tag.

  Op outcome mapping: COMMIT SUCCESS → :ok; a received TransientException
  (write-write conflict, Neo.TransientError.Transaction.Outdated) →
  :fail (definitive abort, ADR-0017/0028 §4); connection loss → :info.

  Why SI ≡ serializable here (the [:serializable] gate): every
  transaction writes everything it reads — the append SET reads the
  entire current list and rewrites it, read set ⊆ write set. Two
  concurrent transactions touching a common key have overlapping write
  sets on that node id, and ADR-0015 §4 first-committer-wins forces one
  to abort. Committed concurrent transactions have pairwise-disjoint
  write sets, no rw anti-dependency edge exists between them, no G2
  cycle is constructible — the SI history is serializable in commit
  order."
  (:require [jepsen [checker :as checker]
                    [generator :as gen]
                    [random :as rand]]
            [jepsen.relate [checker :as cc]
                               [client :as c]
                               [elle :as elle]
                               [workload :as w]]))

(def ^:const create-counter
  "One counter node per key, the append log starting empty."
  "CREATE (:Counter {id: $k, log: []})")

(def ^:const read-log
  "Read the whole current log — the BEGIN-pinned snapshot's version of
  every append so far on this key."
  "MATCH (c:Counter {id: $k}) RETURN c.log AS log")

(def ^:const append-log
  "Append one element: reads the snapshot's log and rewrites it with the
  new element — a read-modify-write of the whole list, which is exactly
  what puts the key in the write set. coalesce guards the empty-store
  case; setup guarantees the node exists. RETURN c.id makes a lost
  append (zero matched rows) die loudly in single-record!."
  "MATCH (c:Counter {id: $k}) SET c.log = coalesce(c.log, []) + $v RETURN c.id AS id")

(def ^:const count-counters
  "Setup verification: exactly the pool's worth of counters."
  "MATCH (c:Counter) RETURN count(c) AS n")

(defn- key-name [i] (str "k" i))

(defn- txn-gen
  "Infinite stream of counter txns. Micro-op budget 1-3 total: 1-3
  appends to distinct keys, plus reads of a subset of those same keys
  (never more than the budget allows) placed immediately before each
  read key's append."
  [key-count]
  (let [keys (mapv key-name (range key-count))
        vctr (atom 0)]
    (repeatedly
     (fn []
       (let [n-appends (inc (rand/long 3))
             n-reads   (rand/long (inc (min n-appends (- 3 n-appends))))
             appended  (take n-appends (rand/shuffle keys))
             read-ks   (set (take n-reads (rand/shuffle appended)))]
         {:type :invoke
          :f :txn
          :value (into []
                       (mapcat (fn [k]
                                 (concat (when (read-ks k) [[:r k nil]])
                                         [[:append k (swap! vctr inc)]])))
                       appended)})))))

(defn- txn!
  "One op = one explicit Bolt transaction executing the micro-ops in
  order. The completed :ok value carries the reads filled in with what
  the transaction observed — the Elle history. A :fail/:info completion
  keeps the invoked value (reads nil), which is exactly the uncertainty
  Elle's :info handling is built for."
  [client _test op]
  (c/explicit-txn!
   client op
   (fn [tx]
     [:commit
      (mapv (fn exec-mop [[f k v :as mop]]
              (case f
                :r      [:r k (vec (.asList (.get (c/single-record! tx read-log {"k" k})
                                                  "log")))]
                :append (do (c/single-record! tx append-log {"k" k "v" (long v)})
                            mop)))
            (:value op))])))

(defn workload
  "The counter lane. opts wants :time-limit, :nemesis (:none default at
  the dispatcher), :nemesis-interval, :op-rate, :counters (4-8), and
  :fault-cycle (nil unless faults were requested)."
  [{:keys [counters fault-cycle] :as opts}]
  {:name (str "relate-counter" (when fault-cycle "-kill9"))
   :client-handlers {:txn txn!}
   :setup (fn setup [uri user password]
            (c/workload-setup!
             uri user password
             {:statements (mapv (fn [i] [create-counter {"k" (key-name i)}])
                                (range counters))
              :verify [count-counters {"n" (long counters)}]}))
   :test-opts {}
   ;; No final read phase: final-state reads happen outside the Elle
   ;; history by design, and the lane has no non-Elle final-state
   ;; checker — the in-txn reads are Elle's inputs.
   :generator (gen/phases
               (w/limited (->> (txn-gen counters)
                               (gen/stagger (/ 1.0 (:op-rate opts))))
                          opts)
               (when fault-cycle (w/stop-faults-phase fault-cycle)))
   :checker (checker/compose
             (cond-> (assoc (elle/counter-checkers)
                            :stats (checker/stats)
                            :client-hygiene (cc/client-hygiene (boolean fault-cycle)))
               fault-cycle (assoc :fault-evidence (cc/fault-evidence {:cycle fault-cycle}))))})
