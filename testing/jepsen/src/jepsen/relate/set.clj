(ns jepsen.relate.set
  "The v1 crash-durability workload's Bolt op handlers (LABS-650):
  :add is one explicit-transaction CREATE of a node with a unique
  user-visible `id` (ack = COMMIT SUCCESS — the ack boundary); :read is
  the final-state read of every surviving node's id. The statements
  are workload.clj's ONE canonical spelling, so every surface running
  the set workload asks the server for byte-identical work. The
  dispatcher (jepsen.relate) builds the set-family lanes — every
  surface, durability lane, and fault schedule — over these handlers;
  the http surface's client carries its own (http_client.clj).

  Keys are the `id` property, never node ids: uncommitted CREATEs
  consume process-wide node ids and rollback leaves gaps (ADR-0028),
  so contiguity is uncheckable by design."
  (:require [jepsen.relate [client :as c]
                               [workload :as w]]))

(defn- add!
  "One explicit-transaction CREATE. :ok only when COMMIT succeeded —
  the ack boundary (see c/explicit-txn!)."
  [client _test op]
  (c/explicit-txn!
   client op
   (fn [tx]
     (.consume (.run tx w/set-add-statement
                     (c/bolt-params {"id" (long (:value op))})))
     [:commit (:value op)])))

(defn- read!
  "The final-state read, with the restart-liveness budget: connection
  errors are retried up to util/read-deadline-secs after the last nemesis
  restart. The recovered id set rides the op as {:ids #{...}} — the
  same shape every surface's final read carries (the http lane's adds
  the register's value)."
  [client _test op]
  (c/final-read!
   client op
   (fn [^org.neo4j.driver.Driver driver]
     (with-open [^org.neo4j.driver.Session session (.session driver)]
       {:ids (set (map (fn [^org.neo4j.driver.Record r] (.asLong (.get r "id")))
                       (.list (.run session w/set-read-statement))))}))))

(def client-handlers
  "The set workload's Bolt op handlers — the one spelling of :add and
  :read every bolt set-family lane runs."
  {:add add! :read read!})
