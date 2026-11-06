(ns jepsen.relate.util
  "One deadline loop (poll-until), shared by every 'poll until it comes
  up' and 'retry connection errors until the budget ends' site in this
  harness: the Bolt and HTTP probes' restart-liveness waits, the docker
  exit polls, and the final reads' retry windows (final-read!, the one
  final-read shape every surface runs). The loop every client was about
  to re-implement.

  The retryable predicate is the caller's policy — `any` for liveness
  polls (any failure just means 'not up yet'), a connection-error
  discriminator for final reads (a definitive rejection must surface
  immediately, not be retried away).")

(defn any
  "Retryable-always: the liveness-poll policy."
  [_t]
  true)

(defn close-quietly!
  "Closes an AutoCloseable, swallowing close-time errors. Close failures
  carry no op outcome — the error that MATTERS is the one that reached
  the catch, and resource cleanup must not replace it."
  [^java.lang.AutoCloseable x]
  (when x
    (try (.close x) (catch Throwable _ nil))))

(defn poll-until
  "Evaluates `(done?)` until it returns truthy or `timeout-secs` (from
  now) pass, sleeping `poll-ms` between attempts. Returns true when the
  condition held inside the budget, false when the deadline expired
  first — the one deadline loop every poll in this harness runs."
  [done? timeout-secs poll-ms]
  (let [deadline (+ (System/nanoTime) (long (* timeout-secs 1e9)))]
    (loop []
      (cond
        (done?)                           true
        (< (System/nanoTime) deadline)    (do (Thread/sleep poll-ms) (recur))
        :else                             false))))

(defn retry-until
  "Calls `(thunk)`; while it throws a `retryable?` Throwable and
  `timeout-secs` (from now) has not passed, sleeps `poll-ms` and tries
  again. Returns the first successful value; throws the last failure
  otherwise — a budget exhaustion is a loud fault, never a nil. Runs
  over poll-until: each attempt is the polled condition."
  [thunk retryable? timeout-secs poll-ms]
  (let [result (volatile! nil)
        attempt (fn []
                  (try (vreset! result {:ok (thunk)})
                       true
                       (catch Throwable t
                         (vreset! result {:error t})
                         (not (retryable? t)))))]
    (poll-until attempt timeout-secs poll-ms)
    (let [r @result]
      (if (contains? r :ok)
        (:ok r)
        (throw (:error r))))))

(def ^:const read-deadline-secs
  "How long a final-state read retries connection errors — the
  restart-liveness budget after the last nemesis restart. A server that
  cannot retake the store within this window fails the run. The one
  budget every surface's final read runs under."
  60)

(defn final-read!
  "The final-state read pattern, every surface's: `read-fn` takes the
  surface's connection (`conn` — the Bolt driver, the HTTP connection
  map) and returns the recovered value. Errors the surface's
  `retryable?` classifies as connection-level retry until
  read-deadline-secs (the restart-liveness budget after the last
  nemesis restart); a definitive FAILURE, or a server that never comes
  back in the budget, is a :fail — the run fails loudly rather than
  reporting 'no final read'."
  [conn op retryable? read-fn]
  (try
    (assoc op :type :ok
           :value (retry-until #(read-fn conn) retryable?
                               read-deadline-secs 1000))
    (catch Throwable t
      (assoc op :type :fail
             :error (str "final read failed after the last restart: "
                         (.getMessage ^Throwable t))))))
