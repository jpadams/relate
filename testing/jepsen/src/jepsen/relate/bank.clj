(ns jepsen.relate.bank
  "The Elle bank lane (LABS-652): classic Jepsen bank — 16-32 :Account
  nodes, a fixed initial total, transfers moving an amount between two
  randomly chosen accounts.

  One Jepsen op = one explicit Bolt transaction = the design's transfer
  shape exactly: read both balances (one statement, the BEGIN-pinned
  snapshot), client-reject (ROLLBACK, no write) unless fb >= amount,
  then write both accounts (one statement: both balances and both
  version tags), COMMIT. Ack boundary COMMIT SUCCESS → :ok;
  Transaction.Outdated at COMMIT → :fail; connection loss → :info.

  Two checkers, two registers, one node each:

  - jepsen's bank checker (there is NO elle.bank namespace): the final
    read must conserve the total exactly and show no negative balance.
    The balances are the business state and live on the node's
    `balance` property.

  - elle.rw-register over the SAME transaction structure (read both,
    write both): the Elle register value is the account's `ver` tag,
    NOT the balance. elle's rw-register assumes unique write values per
    key and hard-asserts (wr-graph) that no read value was written by
    two ops — and balances in a conserved bank repeat by nature (an
    account can hold 95 now and again later), so balance-valued
    registers would abort the checker mid-run on a perfectly legal
    history. The ver tag is drawn from a globally unique counter at
    generation time, so every write is unique by construction; the
    txn structure Elle analyzes (read set {from,to} ⊆ write set
    {from,to}) is the transfer's isolation structure exactly. Write
    values being version tags does not weaken the analysis: every
    dependency edge (wr from the read tag, ww/rw from the inferred
    version order) mirrors the balance's own version order, because
    each committed transfer writes a fresh tag to every account it
    touched.

  Why SI ≡ serializable here (the [:serializable] gate): the transfer
  reads exactly the two accounts it writes — the SETs are
  read-modify-writes on the same pinned snapshot. Concurrent transfers
  sharing an account have overlapping write sets on that node id →
  first-committer-wins aborts the later one (conflict granularity is
  node identity — the granularity.clj measurement settles that
  premise). Committed concurrent transfers touch disjoint account
  pairs → no rw anti-dependency edge → serializes in commit order; in
  any serialization each committed transfer preserved fb >= amt and
  the total, so conservation and non-negativity hold."
  (:require [jepsen [checker :as checker]
                    [generator :as gen]
                    [history :as h]
                    [random :as rand]]
            [jepsen.tests [bank :as bank]]
            [jepsen.relate [checker :as cc]
                               [client :as c]
                               [elle :as elle]
                               [workload :as w]])
  (:import (org.neo4j.driver Record)))

(def ^:const initial-balance
  "Every account starts here; the total is this times the account count."
  100)

(def ^:const create-account
  "One account node: the balance (business state, bank-checked) and the
  ver tag (the Elle register value, unique per write by construction).
  All accounts start at ver 0 — reads of 0 have no ok-writer, which
  elle's wr-graph treats as the initial state."
  "CREATE (:Account {id: $a, balance: $b, ver: 0})")

(def ^:const read-both
  "Read both balances and both version tags in one statement, on the
  transaction's pinned snapshot."
  "MATCH (a:Account {id: $from}), (b:Account {id: $to})
   RETURN a.balance AS fb, b.balance AS tb, a.ver AS fv, b.ver AS tv")

(def ^:const transfer-both
  "Write both accounts in one statement: read-modify-write of both
  balances, plus fresh version tags. RETURN a.id makes a lost write
  (zero matched rows) die loudly in single-record!."
  "MATCH (a:Account {id: $from}), (b:Account {id: $to})
   SET a.balance = a.balance - $amount, a.ver = $nvf,
       b.balance = b.balance + $amount, b.ver = $nvt
   RETURN a.id AS id")

(def ^:const read-accounts
  "The final-state read for the bank checker: every account's balance."
  "MATCH (a:Account) RETURN a.id AS id, a.balance AS balance")

(def ^:const count-accounts
  "Setup verification: exactly the pool's worth of accounts, conserving
  the total."
  "MATCH (a:Account) RETURN count(a) AS n, sum(a.balance) AS total")

(defn- account-name [i] (str "a" i))

(defn- transfer-gen
  "Infinite stream of transfers between two DIFFERENT randomly chosen
  accounts, amount 1..max-transfer, carrying the fresh version tags the
  transaction will write (unique values from one shared counter —
  elle's per-key write-uniqueness, the same discipline as the counter
  lane's appends)."
  [accounts max-transfer]
  (let [vctr (atom 0)]
    (repeatedly
     (fn []
       (let [[from to] (take 2 (rand/shuffle accounts))]
         {:type :invoke
          :f :transfer
          :value {:from           from
                  :to             to
                  :amount         (inc (rand/long max-transfer))
                  :from-write-ver (swap! vctr inc)
                  :to-write-ver   (swap! vctr inc)}})))))

(defn- transfer!
  "One transfer as one explicit Bolt transaction. The observed balances
  and version tags ride every completed op — the :ok value from f, and
  (for a conflict :fail or a connection :info, where the exception path
  loses f's return value) a merge of what the volatile captured before
  the throw. That is what lets ->elle-op derive the attempted writes of
  uncertain outcomes."
  [client _test op]
  (let [observed (volatile! nil)]
    (let [completion
          (c/explicit-txn!
           client op
           (fn [tx]
             (let [{:keys [from to amount from-write-ver to-write-ver]}
                   (:value op)
                   ^Record record (c/single-record!
                                   tx read-both {"from" from "to" to})
                   fb (.asLong (.get record "fb"))
                   tb (.asLong (.get record "tb"))
                   fv (.asLong (.get record "fv"))
                   tv (.asLong (.get record "tv"))
                   obs {:fb fb :tb tb :from-ver fv :to-ver tv}]
               (vreset! observed obs)
               (if (< fb amount)
                 ;; Client rejects: ROLLBACK, no write — a definitive
                 ;; abort, so :fail, with the observation recorded.
                 [:abort (merge (:value op) obs {:rejected true})]
                 (do (c/single-record!
                      tx transfer-both
                      {"from" from "to" to "amount" (long amount)
                       "nvf" (long from-write-ver)
                       "nvt" (long to-write-ver)})
                     [:commit (merge (:value op) obs)])))))]
      (if @observed
        (update completion :value merge @observed)
        completion))))

(defn- read!
  "The final-state read (with the restart-liveness budget): every
  account's balance, as the map jepsen's bank checker consumes."
  [client _test op]
  (c/final-read!
   client op
   (fn [^org.neo4j.driver.Driver driver]
     (with-open [^org.neo4j.driver.Session session (.session driver)]
       (into {}
             (map (fn [^Record r] [(.asString (.get r "id"))
                                   (.asLong (.get r "balance"))]))
             (.list (.run session read-accounts)))))))

(defn- ->elle-op
  "Derives one rw-register txn op from a :transfer op:

    invoke            reads unknown (nil), writes = the fresh tags
    no observation    the connection died before the read returned —
                      no writes were sent either, so reads-only
    :rejected         reads observed, client rolled back before writing
    otherwise         reads observed, both tags written — committed
                      (:ok), definitively discarded (:fail conflict —
                      the writes must never be observed, G1a), or
                      uncertain (:info)"
  [op]
  (let [{:keys [from to from-write-ver to-write-ver from-ver to-ver
                rejected]}
        (:value op)]
    (assoc op
           :f :txn
           :value (cond
                    (= :invoke (:type op))
                    [[:r from nil] [:r to nil]
                     [:w from from-write-ver] [:w to to-write-ver]]

                    (nil? from-ver)
                    [[:r from nil] [:r to nil]]

                    rejected
                    [[:r from from-ver] [:r to to-ver]]

                    :else
                    [[:r from from-ver] [:r to to-ver]
                     [:w from from-write-ver] [:w to to-write-ver]]))))

(defn- ->elle-history
  "The bank history as an elle.rw-register history: every :transfer op
  becomes a :txn op over the two accounts' version registers; the final
  :read ops (and any nemesis ops) are dropped — final-state reads stay
  outside the Elle history by design, and h/client-ops inside elle
  drops the nemesis anyway. The transfer ops are the only ones carrying
  transaction structure."
  [history]
  (h/history (map ->elle-op (h/filter (h/has-f? :transfer) history))))

(defn workload
  "The bank lane. opts wants :time-limit, :nemesis, :nemesis-interval,
  :op-rate, :accounts (16-32), :max-transfer, :fault-cycle."
  [{:keys [accounts max-transfer fault-cycle] :as opts}]
  (let [account-ids (mapv account-name (range accounts))
        total (* (long accounts) initial-balance)]
    {:name (str "relate-bank" (when fault-cycle "-kill9"))
     :client-handlers {:transfer transfer! :read read!}
     :setup (fn setup [uri user password]
              (c/workload-setup!
               uri user password
               {:statements (mapv (fn [a] [create-account
                                           {"a" a "b" (long initial-balance)}])
                                  account-ids)
                :verify [count-accounts {"n" (long accounts)
                                         "total" total}]}))
     ;; jepsen's bank checker reads these off the test map.
     :test-opts {:accounts account-ids :total-amount total}
     :generator (gen/phases
                 (w/limited (->> (transfer-gen account-ids max-transfer)
                                 (gen/stagger (/ 1.0 (:op-rate opts))))
                            opts)
                 (when fault-cycle (w/stop-faults-phase fault-cycle))
                 (w/final-read-phase))
     :checker (checker/compose
               (cond-> (assoc (elle/bank-checkers ->elle-history)
                              :stats (checker/stats)
                              :bank (bank/checker {:negative-balances? false})
                              :client-hygiene (cc/client-hygiene (boolean fault-cycle)))
                 fault-cycle (assoc :fault-evidence (cc/fault-evidence {:cycle fault-cycle}))))}))
