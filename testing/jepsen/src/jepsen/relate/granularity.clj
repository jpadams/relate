(ns jepsen.relate.granularity
  "The conflict-granularity measurement (LABS-652): do two concurrent
  explicit transactions SETting DIFFERENT properties of the SAME node
  conflict, or do they commit independently?

  ADR-0015 §4 reads as identity granularity — the WAL's write-set
  conflict check is on node/rel ID, first-committer-wins — so the
  expected answer is CONFLICT: the second COMMIT gets
  Neo.TransientError.Transaction.Outdated. This program settles the
  question by measurement instead of reading, with a control scenario
  proving the rig can observe conflicts at all.

  Scenario (deterministic interleaving, no races — the window is held
  open by the transactions themselves, exactly the Elle lanes' shape;
  the two transactions run on two separate sessions, since one Bolt
  session carries one open transaction):

    1. fresh probe node {a: 0, b: 0}
    2. txn A (session A) begins and reads the probe   (pins A's snapshot)
    3. txn B (session B) begins, SETs property b, commits (SUCCESS expected)
    4. txn A SETs property a, commits                 ← THE MEASUREMENT:
       Transaction.Outdated ⇒ identity granularity (conflicting);
       SUCCESS              ⇒ property granularity (independent)

  Control scenario: identical, but both transactions SET property a —
  the same-property write-write conflict the engine documents
  (ADR-0015 §4); a control whose second commit SUCCEEDS would mean the
  rig cannot observe conflicts and the measurement certifies nothing.

  Run (control container, DB up):

    clojure -M:granularity bolt://db:7687 jepsen jepsen
    clojure -M:granularity --help"
  (:require [jepsen.relate [client :as c]
                               [util :as util]])
  (:import (org.neo4j.driver Session Transaction)))

(def ^:const create-probe
  "A fresh probe node per iteration: id unique, both properties zero."
  "CREATE (:GranularityProbe {id: $id, a: 0, b: 0})")

(def ^:const read-probe
  "The snapshot-pinning read (the Elle lanes' read-before-write shape)."
  "MATCH (p:GranularityProbe {id: $id}) RETURN p.a AS a, p.b AS b")

(def ^:const set-a
  "Write property a only."
  "MATCH (p:GranularityProbe {id: $id}) SET p.a = $v RETURN p.id AS id")

(def ^:const set-b
  "Write property b only."
  "MATCH (p:GranularityProbe {id: $id}) SET p.b = $v RETURN p.id AS id")

(def ^:const iterations 5)

(defn- commit-outcome
  "Commits an open transaction and returns the outcome with evidence:
  :committed, :outdated (the write-write conflict — TransientError,
  whose Neo4j status code is recorded verbatim), or :error (anything
  else — reported, never interpreted)."
  [^Transaction tx]
  (try
    (.commit tx)
    {:outcome :committed}
    (catch org.neo4j.driver.exceptions.TransientException t
      {:outcome :outdated :code (.code t) :message (.getMessage t)})
    (catch Throwable t
      {:outcome :error :message (.getMessage t)})))

(defn- scenario
  "Runs one interleaved pair against a fresh probe node and returns both
  commit outcomes. `b-stmt` is what B SETs; `a-stmt` is what A SETs (the
  control passes set-a for both; the question passes set-b/set-a)."
  [^Session session-a ^Session session-b id b-stmt a-stmt]
  ;; 1. fresh probe (implicit txn — setup, not measurement)
  (.consume (.run session-a create-probe (c/bolt-params {"id" id})))
  ;; 2. A begins + reads (pins the snapshot)
  (let [tx-a (.beginTransaction session-a)
        _ (c/single-record! tx-a read-probe {"id" id})
        ;; 3. B: own txn on its own session, SET + COMMIT (SUCCESS expected)
        tx-b (.beginTransaction session-b)
        _ (c/single-record! tx-b b-stmt {"id" id "v" (long 2)})
        b-out (commit-outcome tx-b)
        ;; 4. A: SET its property + COMMIT — the measurement.
        _ (c/single-record! tx-a a-stmt {"id" id "v" (long 1)})
        a-out (commit-outcome tx-a)]
    (util/close-quietly! tx-a)
    (util/close-quietly! tx-b)
    {:b b-out :a a-out}))

(defn -main
  "Runs the two scenarios `iterations` times each against the DB and
  prints the measured answer with the reproducing scenario. Exit 0 when
  every iteration agreed AND the control conflicted (either verdict is a
  valid measurement — the ADR's expectation is not baked in); exit 1
  otherwise."
  [& args]
  (when (contains? (set args) "--help")
    (println "usage: clojure -M:granularity [bolt-uri user password]")
    (System/exit 0))
  (let [[uri user password] (case (count args)
                              0 ["bolt://db:7687" "jepsen" "jepsen"]
                              3 args
                              (do (println "expected 0 or 3 args:"
                                           "[bolt-uri user password]")
                                  (System/exit 254)))]
    (with-open [driver (c/driver* uri user password)]
      (c/assert-identity! (c/server-agent driver))
      (with-open [^Session session-a (.session driver)
                  ^Session session-b (.session driver)]
        (let [run (fn run-scenario [label b-stmt a-stmt]
                    (println (str "\n== " label " =="))
                    (mapv (fn [i]
                            (let [id (str label "-" i)
                                  r (scenario session-a session-b
                                              id b-stmt a-stmt)]
                              (println "  iteration" i
                                       "| B (first committer):"
                                       (:outcome (:b r))
                                       "| A (second committer):"
                                       (:outcome (:a r))
                                       (when-let [code (:code (:a r))]
                                         (str "(" code ")")))
                              r))
                          (range iterations)))
              different (run "different-properties" set-b set-a)
              same (run "same-properties-control" set-a set-a)
              a-outcomes (mapv #(-> % :a :outcome) different)
              verdict (cond
                        (every? #{:outdated} a-outcomes) :identity-granularity
                        (every? #{:committed} a-outcomes) :property-granularity
                        :else :mixed)]
          (println)
          (println "B (first committer) in every iteration:"
                   (distinct (mapv #(-> % :b :outcome) (concat different same))))
          (case verdict
            :identity-granularity
            (println "MEASURED: CONFLICT — two transactions SETting different"
                     "properties of the same node conflict; the second COMMIT"
                     "fails with" (-> different first :a :code)
                     "(Transaction.Outdated). Conflict granularity is NODE"
                     "IDENTITY, confirming the ADR-0015 §4 reading.")

            :property-granularity
            (println "MEASURED: INDEPENDENT — two transactions SETting different"
                     "properties of the same node both commit; conflict"
                     "granularity is PER-PROPERTY, refuting the ADR-0015 §4"
                     "identity reading.")

            (println "MEASURED: MIXED/ERROR — outcomes disagreed across"
                     "iterations; treat as inconclusive and read the"
                     "per-iteration evidence above."))
          (let [control-conflicted? (every? #{:outdated}
                                            (mapv #(-> % :a :outcome) same))]
            (println "Control (same property):"
                     (if control-conflicted?
                       "conflicted as documented — the rig observes conflicts."
                       (str "DID NOT CONFLICT — the rig cannot observe "
                            "conflicts; the measurement above certifies "
                            "nothing.")))
            (System/exit (if (and (contains? #{:identity-granularity
                                               :property-granularity}
                                            verdict)
                                  control-conflicted?
                                  ;; B must have committed in every iteration,
                                  ;; or A never faced a committed conflict.
                                  (every? #(= :committed (:outcome (:b %)))
                                          (concat different same)))
                          0 1))))))))
