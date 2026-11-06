(ns jepsen.relate.http-client
  "The Jepsen client for Relate's HTTP Query API v2 surface
  (LABS-655).

  CLIENT CHOICE, verified against docs/api/http/spec.md: no official
  Java-driver version speaks this surface. neo4j-java-driver 5.28.5 —
  the latest on Maven Central at 2026-11-01, and the version the Bolt
  lane pins — is Bolt-only: its URI scheme table
  (org.neo4j.driver.internal.Scheme) enumerates exactly bolt, bolt+s,
  bolt+ssc, neo4j, neo4j+s, neo4j+ssc, and neither it nor anything else
  in the org.neo4j.driver group ships a Query API HTTP transport. So
  this lane speaks the wire itself: the JDK's java.net.http for HTTP/1.1
  and clojure/data.json for the envelopes, with zero other deps.

  Wire contract (docs/api/http/spec.md, ADR-0232):

  - One request per connection (`connection: close`); no session state.
    Every op is a fresh `POST /db/neo4j/query/v2` — the auto-commit
    statement endpoint (explicit transactions are LABS-647, out of
    scope). Request bodies use `Content-Type: application/json`, the
    plain-parameter dialect; `Accept: application/json` selects the
    plain response encoding. No `Origin` header and a non-simple
    content type, so the browser guard admits the request (ADR-0425).
  - THE ACK BOUNDARY IS THE CONSUMED 200 RESPONSE. An op is :ok only
    when a complete HTTP 200 envelope was parsed — for an auto-commit
    route that is 'the write committed'. Any other status carrying a
    parsed error envelope is a definitive :fail: the server answered,
    and auto-commit is atomic per request, so a definitive answer means
    the statement did not commit. A transport-level failure (connect
    refused, timeout, reset mid-response) is :info — the response never
    arrived and the commit state is unknown. The HTTP surface has no
    server agent string to pin like Bolt's; the identity gate here is
    the discovery document: `GET /` must advertise a query endpoint
    ending in the v2 route (asserted at client open and in every probe).
  - The kill nemesis verifies faults on THIS wire: a kill must make the
    query route unreachable, a restart must bring it back (nemesis.clj
    dispatches per surface)."
  (:require [clojure.data.json :as json]
            [clojure.string :as str]
            [jepsen [client :as client]]
            [jepsen.relate [util :as util]
                               [workload :as wl]])
  (:import (java.net URI)
           (java.net.http HttpClient HttpClient$Version
                          HttpRequest HttpRequest$BodyPublishers
                          HttpResponse HttpResponse$BodyHandlers)
           (java.time Duration)
           (java.util Base64)))

(def ^:const query-path
  "The auto-commit statement route — the fixed `neo4j` namespace
  (ADR-0116) under the Query API v2 path (docs/api/http/spec.md)."
  "/db/neo4j/query/v2")

(def ^:const connect-timeout-secs
  "Short on purpose: a probe against a dead container must fail in
  seconds, not the JDK default, so nemesis fault verification and :info
  classification stay crisp."
  5)

(def ^:const request-timeout-secs
  "Per-request budget: auto-commit statements are fast; a request hung
  past this is a transport-level failure — :info, never a hang."
  10)

(defn transport-error?
  "The failures that mean 'unknown whether the op committed': the wire
  died or the server was unreachable. Every java.net.http failure mode
  this lane can meet — connect refused, connect timeout, response
  timeout, reset mid-response — is an IOException."
  [^Throwable t]
  (instance? java.io.IOException t))

(defn http-client*
  "An HttpClient pinned to HTTP/1.1 (the surface's only version) with
  the short connect timeout."
  ^HttpClient []
  (-> (HttpClient/newBuilder)
      (.version HttpClient$Version/HTTP_1_1)
      (.connectTimeout (Duration/ofSeconds connect-timeout-secs))
      (.build)))

(defn conn*
  "A connection map: a fresh HttpClient plus the target and credential.
  The shared spelling for the standalone calls (probe, setup write) that
  live outside any client record."
  [uri user password]
  {:http (http-client*) :uri uri :user user :password password})

(defn- basic-auth
  "The RFC 7617 Authorization header value."
  [user password]
  (str "Basic "
       (.encodeToString (Base64/getEncoder)
                        (.getBytes (str user ":" password) "UTF-8"))))

(defn- parse-body
  "Parses a response body as JSON, or nil when the body is empty — an
  honest absence the callers report alongside the status, never a
  fabricated envelope."
  [^String body]
  (when-not (str/blank? body)
    (json/read-str body :key-fn keyword)))

(defn- request
  "Builds the statement POST."
  [{:keys [uri user password]} statement parameters]
  (-> (HttpRequest/newBuilder (URI/create (str uri query-path)))
      (.header "Authorization" (basic-auth user password))
      (.header "Content-Type" "application/json")
      (.header "Accept" "application/json")
      (.timeout (Duration/ofSeconds request-timeout-secs))
      (.POST (HttpRequest$BodyPublishers/ofString
              (json/write-str (cond-> {:statement statement}
                                parameters (assoc :parameters parameters)))))
      (.build)))

(defn post-statement*
  "One auto-commit statement POST. Returns {:status, :body} for ANY
  HTTP response — 2xx or error envelope alike; throws only on
  transport-level failure. The classification decision belongs to the
  caller, whose ack boundary differs (an op :fail vs a read's loud
  throw)."
  [conn statement parameters]
  (let [^HttpResponse resp (.send (:http conn)
                                  (request conn statement parameters)
                                  (HttpResponse$BodyHandlers/ofString))]
    {:status (.statusCode resp)
     :body   (parse-body (.body resp))}))

(defn error-summary
  "One-line rendering of an error envelope for op :error fields and
  thrown failures."
  [status body]
  (let [e (first (:errors body))]
    (str "HTTP " status (when e (str " " (:code e) ": " (:message e))))))

(defn post-statement!
  "Like post-statement*, but a 200-with-envelope is the only acceptable
  answer — anything else throws. For the calls where a non-200 is a
  fault, not a classification: probes, setup writes, the final read."
  [conn statement parameters]
  (let [{:keys [status body]} (post-statement* conn statement parameters)]
    (when-not (and (= 200 status) (contains? body :data))
      (throw (ex-info (str "query route answer is not a 200 envelope: "
                           (error-summary status body) "; body " (pr-str body))
                      {:status status :body body})))
    body))

(defn- discovery*
  "GET / — the public discovery document (docs/api/http/spec.md)."
  [{:keys [^HttpClient http uri]}]
  (let [req (-> (HttpRequest/newBuilder (URI/create (str uri "/")))
                (.timeout (Duration/ofSeconds request-timeout-secs))
                (.GET)
                (.build))
        resp (.send http req (HttpResponse$BodyHandlers/ofString))]
    {:status (.statusCode resp)
     :body   (parse-body (.body resp))}))

(def ^:const discovery-query-suffix
  "What the discovery document's `query` URL must end with — the
  templated v2 route (docs/api/http/spec.md: the concrete
  `{databaseName}` is the client's to substitute; the probe POST already
  proves the concrete route serves)."
  "/db/{databaseName}/query/v2")

(defn- assert-discovery!
  "Throws unless the discovery document is a 200 advertising a query
  endpoint ending in the templated v2 route — this surface's identity
  gate (the analog of the Bolt lane's verbatim agent pin). Returns the
  query endpoint URL, the identity string the probes carry."
  [{:keys [status body]}]
  (let [q (:query body)]
    (if (and (= 200 status) (string? q)
             (str/ends-with? q discovery-query-suffix))
      q
      (throw (ex-info (str "discovery document mismatch: expected 200 with a "
                           "query endpoint ending " discovery-query-suffix
                           ", got status " status " body " (pr-str body))
                      {:status status :body body})))))

(defn probe!
  "One probe round trip: the statement POST (liveness + auth + statement
  service, its answer verified) then the discovery document (identity).
  Returns the query endpoint URL; throws on any failure. The one shared
  spelling of 'is the DB alive and is it ours' on this surface — used at
  DB setup and after every nemesis restart."
  [uri user password]
  (let [conn (conn* uri user password)
        body (post-statement! conn wl/probe-statement nil)]
    (when-not (= {:fields ["one"] :values [[1]]} (:data body))
      (throw (ex-info (str "probe answer mismatch: " (pr-str body)) {:body body})))
    (assert-discovery! (discovery* conn))))

(defn wait-up!
  "Polls probe! every 500ms until it succeeds or `timeout-secs` passes.
  Returns the query endpoint URL; throws the last failure otherwise. The
  shared restart-liveness wait (DB setup, nemesis restart)."
  [uri user password timeout-secs]
  (util/retry-until #(probe! uri user password)
                    util/any timeout-secs 500))

(defn init-register!
  "The register workload's setup write (workload.clj): places the
  register node and its initial value on the freshly wiped store, before
  any client connects. Throws on any failure — a lost setup write is a
  setup fault, never something to paper over."
  [uri user password]
  (post-statement! (conn* uri user password)
                   wl/register-init-statement
                   {"key" wl/register-key
                    "initial" wl/register-initial-value}))

(defn- invoke-statement!
  "One auto-commit statement op (set add or register write), classified
  per the ack boundary: consumed 200 = :ok, any other parsed response =
  definitive :fail, transport-level failure = :info."
  [conn _test op statement parameters]
  (try
    (let [{:keys [status body]} (post-statement* conn statement parameters)]
      (if (= 200 status)
        (assoc op :type :ok)
        (assoc op :type :fail :error (error-summary status body))))
    (catch Throwable t
      (assoc op :type :info :error (.getMessage ^Throwable t)))))

(defn- read-final*
  "The recovered state in one pass: every surviving set id, and the
  register's value — asserted to come from exactly one register node.
  Throws on any failure."
  [conn]
  (let [set-body  (post-statement! conn wl/set-read-statement nil)
        ids       (set (map first (get-in set-body [:data :values])))
        reg-body  (post-statement! conn wl/register-read-statement
                                   {"key" wl/register-key})
        reg-rows  (get-in reg-body [:data :values])]
    (when-not (= 1 (count reg-rows))
      (throw (ex-info (str "register read returned " (count reg-rows)
                           " rows — expected exactly one register node; rows "
                           (pr-str reg-rows))
                      {:rows reg-rows})))
    {:ids ids :register (first (first reg-rows))}))

(defn- read!
  "The final-state read, through util/final-read! (the one final-read
  shape, util/read-deadline-secs's restart-liveness budget) with
  transport-error? as the retryable class. Any definitive failure —
  including a register that vanished (the acked setup write was lost) —
  is a loud :fail, never a 'no final read'."
  [conn _test op]
  (util/final-read! conn op transport-error? read-final*))

(defrecord QueryApiClient [^HttpClient http uri user password]
  client/Client
  (open! [this _test _node]
    (let [http (http-client*)]
      ;; Pin the transport premise at every client: the discovery
      ;; document must advertise the Query API v2 route (the surface's
      ;; identity gate).
      (assert-discovery! (discovery* {:http http :uri uri}))
      (assoc this :http http)))

  (setup! [this _test])

  (invoke! [this test op]
    (case (:f op)
      :add   (invoke-statement! this test op wl/set-add-statement
                                {"id" (:value op)})
      :write (invoke-statement! this test op wl/register-write-statement
                                {"key" wl/register-key
                                 "value" (:value op)})
      :read  (read! this test op)))

  (teardown! [this _test])

  (close! [_this _test]
    (util/close-quietly! http))

  ;; Reusable after an :info op: every request is a fresh connection, so
  ;; a crashed worker's next op opens a new one — no client churn.
  client/Reusable
  (reusable? [_this _test] true))
