(ns jepsen.relate.workload
  "The workload statements and value space, shared by every surface lane
  (Bolt, HTTP Query API v2): ONE canonical spelling of the set and
  register workloads, so two lanes running the 'same' workload are
  byte-identical in what they ask the server to do.

  The set workload keys on the user-visible `id` property, never node
  ids: uncommitted CREATEs consume process-wide node ids and rollback
  leaves gaps (ADR-0028), so id contiguity is uncheckable by design.

  The register workload is one durable register — a single
  :JepsenRegister node identified by `register-key`, last write wins.
  Its initial value is a SETUP write the DB layer places after the
  hermetic wipe and before any client connects (never a client op: five
  clients MERGE-ing concurrently against an empty store could each
  not-find and create; one setup MERGE on the empty store cannot race),
  and with the node existing, the single-writer commit loop serializes
  every later MERGE onto a match. The final read asserts exactly one
  register node exists — a duplicate is a finding, never something to
  absorb silently.

  Shared phase machinery: the concurrent phase is 'client ops,
  rate-limited, cut TOGETHER with the nemesis cycle by the time limit'
  — the limit must wrap the combined generator, or a nemesis cycle
  would never exhaust and the run would never end. Workload namespaces
  receive opts with :fault-cycle already resolved (nil when the lane
  runs without faults — the Elle lanes' default, the MySQL 8.0.34
  isolation-under-concurrency precedent) and build their generators
  from these pieces, so every lane shares one phase shape. A fault
  cycle is DATA — the vector of nemesis :f steps, fault first, recovery
  last (the Elle kill toggle's [:start :stop], a schedule row's :cycle);
  the helpers below turn it into the generator pieces."
  (:require [jepsen [generator :as gen]]))

(def ^:const probe-statement
  "The liveness probe statement shared by every surface client: one
  cheap round trip whose answer is fully known."
  "RETURN 1 AS one")

(def ^:const set-add-statement
  "One node with a unique user-visible `id` property."
  "CREATE (:JepsenSet {id: $id})")

(def ^:const set-read-statement
  "The final-state set read: every surviving node's `id`."
  "MATCH (n:JepsenSet) RETURN n.id AS id")

(def ^:const register-key
  "The one register's identity property value."
  "reg")

(def ^:const register-initial-value
  "The value the setup write places — the register's state before any
  client write, and the one legal final value when no write landed."
  0)

(def ^:const register-init-statement
  "The register's setup write, placed by DB setup on the freshly wiped
  store: MERGE (idempotent — a clean run's first write, a residue's
  no-op) so restarts of the setup path never duplicate the register."
  "MERGE (r:JepsenRegister {key: $key}) ON CREATE SET r.value = $initial")

(def ^:const register-write-statement
  "One register write: last write wins. The MERGE matches the setup
  node (it exists before any client op); the SET is the write itself."
  "MERGE (r:JepsenRegister {key: $key}) SET r.value = $value")

(def ^:const register-read-statement
  "The final-state register read: the one register's current value."
  "MATCH (r:JepsenRegister {key: $key}) RETURN r.value AS value")

(defn fault-cycle
  "The nemesis generator over a fault cycle's steps: a nemesis-interval
  sleep, then the step's op, through the cycle, forever — until the
  time limit cuts the combined generator. A Clojure seq is itself a
  generator (io.jepsen/generator)."
  [steps nemesis-interval]
  (cycle (mapcat (fn [f] [(gen/sleep nemesis-interval)
                          {:type :info :f f}])
                 steps)))

(defn limited
  "The concurrent phase: ops at the lane's rate, cut by the time limit —
  together with the fault cycle when one is in play. Without faults the
  ops still route to CLIENTS only: an unwrapped generator serves every
  process, and the (idle) nemesis worker would draw client ops it has
  no clause for."
  [ops {:keys [time-limit nemesis-interval] steps :fault-cycle}]
  (if steps
    (->> ops
         (gen/nemesis (fault-cycle steps nemesis-interval))
         (gen/time-limit time-limit))
    (->> (gen/clients ops) (gen/time-limit time-limit))))

(defn stop-faults-phase
  "The cycle's LAST step once more — always the recovery op, so the DB
  is guaranteed up no matter where the time limit cut the cycle (if it
  ended on a fault, the DB is still down or frozen)."
  [steps]
  (gen/nemesis (gen/once {:type :info :f (last steps)})))

(defn final-read-phase
  "One client performs the final read."
  []
  (gen/clients (gen/once {:type :invoke :f :read})))
