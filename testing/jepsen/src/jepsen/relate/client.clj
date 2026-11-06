(ns jepsen.relate.client
  "The shared Bolt transport for every Jepsen workload: plain Java interop
  over the official neo4j-java-driver (5.28.5), the exact transport the
  LABS-649 smoke pinned (surfaces/relate-bolt/tests/java_driver_smoke).

  This namespace owns the transport and the op-outcome taxonomy; the
  workload shapes (statements, generators, checkers) live in the workload
  namespaces (set / counter / bank) and reach the wire only through the
  helpers here.

  Three rules are load-bearing and must never regress:

  - EXPLICIT TRANSACTIONS ONLY. The driver's managed executeWrite
    auto-retries TransientException — and Relate deliberately maps
    write-write conflict onto exactly that classification so drivers
    retry it (docs/api/bolt/spec.md, ADR-0307). A managed-transaction
    client would silently retry away the conflict behavior the
    transactional lanes (LABS-652) exist to observe.

  - THE ACK BOUNDARY IS COMMIT. An op is :ok only when the explicit
    transaction's COMMIT succeeded; a received FAILURE (any Neo4jException
    that is not a connection-level failure) is a definitive :fail; a
    connection-level error is :info — uncertain, the op may or may not
    have committed (docs/api/bolt/spec.md, ADR-0319: failures are
    reported at PULL/DISCARD or COMMIT, not RUN).

  - NO SILENT FALLBACKS. A statement that returns zero or many rows where
    the workload's premise says exactly one is a broken premise
    (single-record! throws), and an exception the client cannot classify
    as a server FAILURE or a connection error is a harness error — the
    conservative :info claim rides the op, but the :error-class marker
    lets the client-hygiene checker fail the run on it.

  Identity is pinned at every connection: driver 5.x validates the HELLO
  server agent and rejects anything not starting `Neo4j/` (the LABS-649
  finding), and this harness asserts the agent is bolt-identity
  VERBATIM — the driver only checks the prefix, the verbatim check is
  ours."
  (:require [jepsen [client :as client]]
            [jepsen.relate [util :as util]
                               [workload :as wl]])
  (:import (java.util ArrayList)
           (java.util.concurrent TimeUnit TimeoutException)
           (clojure.lang ExceptionInfo)
           (org.neo4j.driver AuthTokens Config Driver GraphDatabase
                             QueryRunner Record Session)
           (org.neo4j.driver Values)
           (org.neo4j.driver.exceptions Neo4jException
                                       ServiceUnavailableException
                                       SessionExpiredException
                                       TransientException)))

(def ^:const bolt-identity
  "The server agent the DB container must present verbatim — the value
  its `serve --bolt-identity` flag is pinned to. Part of the harness's
  contract with the official driver (LABS-649), asserted at client
  setup, at DB setup, and after every nemesis restart."
  "Neo4j/5.26.0")

(def ^:const connection-timeout-secs
  "Short on purpose: a probe against a dead container must fail in
  seconds, not the driver's 30s default, so nemesis fault verification
  and :info classification stay crisp."
  5)

(def ^:const acquisition-timeout-secs 10)

(defn driver*
  "A Driver with short connection timeouts."
  ^Driver [^String uri user password]
  (GraphDatabase/driver
   uri
   (AuthTokens/basic user password)
   (-> (Config/builder)
       (.withConnectionTimeout connection-timeout-secs TimeUnit/SECONDS)
       (.withConnectionAcquisitionTimeout acquisition-timeout-secs TimeUnit/SECONDS)
       (.build))))

(defn server-agent
  "Runs the probe statement over a fresh session and returns the HELLO
  server agent string — one round trip that proves both liveness and
  identity."
  [^Driver driver]
  (with-open [^Session session (.session driver)]
    (let [result (.run session wl/probe-statement)]
      (.agent (.server (.consume result))))))

(defn assert-identity!
  "Throws unless the server agent is exactly bolt-identity. Verbatim —
  the driver's own gate only requires the `Neo4j/` prefix; the exact
  value is this harness's pin."
  [agent]
  (when-not (= bolt-identity agent)
    (throw (ex-info (str "server identity mismatch: expected "
                         bolt-identity " verbatim, got " (pr-str agent)
                         " — serve must run with --bolt-identity "
                         bolt-identity)
                    {:expected bolt-identity :got agent})))
  agent)

(defn probe!
  "Fresh driver, one probe round trip, pinned identity. Returns the
  agent string; throws on any failure. The one shared spelling of 'is
  the DB alive and is it ours' — used at client setup, at DB setup, and
  after every nemesis restart."
  [uri user password]
  (with-open [^Driver driver (driver* uri user password)]
    (assert-identity! (server-agent driver))))

(defn probe-unchecked!
  "Like probe!, but returns the HELLO server agent without asserting it
  verbatim — for servers whose agent string this harness does not own:
  the compare workload's real-Neo4j reference container (LABS-657) may
  spell its agent differently than the pinned identity we force onto
  Relate's serve."
  [uri user password]
  (with-open [^Driver driver (driver* uri user password)]
    (server-agent driver)))

(defn wait-up!
  "Polls probe! every 500ms until it succeeds or `timeout-secs` passes.
  Returns the agent; throws the last failure otherwise. The shared
  restart-liveness wait (DB setup, nemesis restart) — util/retry-until
  with the retryable-always policy: any failure just means 'not up
  yet'."
  [uri user password timeout-secs]
  (util/retry-until #(probe! uri user password)
                    util/any timeout-secs 500))

(defn connection-error?
  "The exceptions that mean 'unknown whether the op committed': the wire
  died or the server is unreachable. Everything else Neo4j-shaped is a
  received FAILURE — definitive."
  [^Throwable t]
  (or (instance? ServiceUnavailableException t)
      (instance? SessionExpiredException t)
      (instance? java.io.IOException t)
      (instance? TimeoutException t)))

(defn classify-error
  "Maps an exception thrown during an op to a [completion-type
  error-class] pair. The type is the honest claim about the commit:
  TransientException is a definitive :fail, not an :info — it is a
  received FAILURE on the wire (write-write conflict surfaces as
  Neo.TransientError.Transaction.Outdated; the server itself never
  retries, ADR-0017/0307) — the op definitively did not commit. Only
  connection-level errors are uncertain.

  The class is evidence for the client-hygiene checker: :server (a
  received FAILURE, always a legal outcome), :connection (uncertain,
  legal only under a nemesis), :harness (this client's own invariant
  guards or an unrecognized throw — never a legal workload outcome)."
  [^Throwable t]
  (cond
    (instance? TransientException t) [:fail :server]
    (connection-error? t)            [:info :connection]
    (instance? Neo4jException t)     [:fail :server]
    :else                            [:info :harness]))

(defn- java-list
  "A Clojure sequential as a java.util.ArrayList, recursively — the
  driver's value mapper takes java Lists; scalars pass through."
  [v]
  (if (sequential? v)
    (ArrayList. ^java.util.Collection (mapv java-list v))
    v))

(defn bolt-params
  "Flattens a map into the Object[] of alternating name/value pairs that
  Values/parameters (a Java varargs static) wants: keys by name (string
  or keyword), nested sequential values as java Lists. The one
  Clojure-map → Bolt-parameters conversion every workload and the
  compare lane's differential engine send through."
  [m]
  (Values/parameters (into-array Object (mapcat (fn [[k v]] [(name k) (java-list v)])
                                                m))))

(defn single-record!
  "Runs stmt (with params) on a Session or Transaction and returns THE
  record: exactly one row, or a loud throw. Workload statements match one
  node by a unique key — zero or many rows means the premise broke, and
  that is never something to paper over. The throw rolls the txn back
  (explicit-txn!'s finally) so :fail stays honest."
  [^QueryRunner runner stmt params]
  (let [records (.list (.run runner stmt (bolt-params params)))]
    (case (.size records)
      1 (.get records 0)
      (throw (ex-info (str "statement matched " (.size records)
                           " rows, expected exactly 1: " stmt)
                      {:stmt stmt :rows (.size records)})))))

(defn explicit-txn!
  "Runs (f tx) as ONE explicit Bolt transaction on a fresh session of
  the client's driver and completes the op. f returns [action value]:

    [:commit value] — COMMIT; :ok only when COMMIT itself succeeded (the
                     ack boundary). value rides the op verbatim.
    [:abort  value] — ROLLBACK: the client chose not to write (the bank
                     lane's insufficient-balance rejection); definitive
                     :fail, value rides the op.

  f throwing rolls the transaction back (the finally close) and the
  error is classified: a Neo4jException is a received FAILURE (:fail,
  :server), a connection error is :info, anything else is a harness
  error (:info claim, :harness class). Transaction/Session close is
  guarded (close-quietly!) so a dead connection at cleanup cannot mask
  the original, classifiable error."
  [{:keys [^Driver driver]} op f]
  (try
    (let [session (.session driver)]
      (try
        (let [tx (.beginTransaction session)]
          (try
            (let [[action value] (f tx)]
              (case action
                :commit (do (.commit tx) (assoc op :type :ok :value value))
                :abort  (do (.rollback tx) (assoc op :type :fail :value value))))
            (finally (util/close-quietly! tx))))
        (finally (util/close-quietly! session))))
    (catch Throwable t
      (let [[type class] (classify-error t)]
        (assoc op :type type
               :error (.getMessage ^Throwable t)
               :error-class class)))))

(defn final-read!
  "The Bolt binding of util/final-read! (the one final-read shape):
  read-fn takes the client's driver and returns the recovered value;
  connection-error? is the retryable class."
  [{:keys [^Driver driver]} op read-fn]
  (util/final-read! driver op connection-error? read-fn))

(defn workload-setup!
  "Runs a workload's Bolt-level setup (counter keys, bank accounts) over
  one identity-pinned driver: each [stmt params] pair in its own implicit
  transaction (a session run without BEGIN), then — when :verify is
  given — the verification statement must return exactly one record
  whose named fields, read as longs, equal the expected map. A silently
  no-op CREATE dies there, loudly: the wipe/setup pair must leave
  exactly the workload's nodes or every subsequent op is meaningless."
  [uri user password {:keys [statements verify]}]
  (with-open [^Driver driver (driver* uri user password)]
    (assert-identity! (server-agent driver))
    (with-open [^Session session (.session driver)]
      (doseq [[stmt params] statements]
        (.consume (.run session stmt (bolt-params params))))
      (when verify
        (let [[stmt expected] verify
              record (single-record! session stmt {})]
          (doseq [[field want] expected]
            (let [got (.asLong (.get ^Record record (name field)))]
              (when-not (= (long want) got)
                (throw (ex-info (str "workload setup verification failed: "
                                     (name field) " = " got ", expected "
                                     (long want) " — " stmt)
                                {:stmt stmt :field (name field)
                                 :expected (long want) :got got}))))))))))

(defrecord BoltClient [^Driver driver uri user password handlers]
  client/Client
  (open! [this _test node]
    (let [driver (driver* uri user password)]
      ;; Pin the transport premise at every client: the HELLO server
      ;; agent must be bolt-identity verbatim (LABS-649).
      (assert-identity! (server-agent driver))
      (assoc this :driver driver)))

  (setup! [this _test])

  (invoke! [this test op]
    (let [f (get handlers (:f op))]
      (when-not f
        (throw (ex-info (str "no client handler for :f " (pr-str (:f op)))
                        {:f (:f op)})))
      (f this test op)))

  (teardown! [this _test])

  (close! [this _test]
    (util/close-quietly! driver))

  ;; Reusable after an :info op: the driver's pool re-establishes dead
  ;; connections on the next session, so a crashed worker keeps its
  ;; client instead of churning driver objects.
  client/Reusable
  (reusable? [this _test] true))
