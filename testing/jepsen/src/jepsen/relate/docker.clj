(ns jepsen.relate.docker
  "The docker socket is this harness's control plane.

  The DB node is the stock relate-bolt container (compose service
  `db`): scratch-based, no shell, no SSH, entrypoint = PID 1. There is no
  jepsen.control channel into it at all — every lifecycle action (wipe,
  start, kill, SIGTERM, pause, unpause, restart, one-shot probe
  containers) is a docker CLI call from the control container, and every
  observation (container state, logs) comes back the same way. The data
  plane is the Bolt wire.

  Every wrapper here fails loudly: a docker call that silently no-ops
  would read as a green run — the exact failure mode that made jepsen
  0.3.12-0.3.14's grepkill! nemeses meaningless."
  (:require [clojure.java.shell :refer [sh]]
            [clojure.string :as str]
            [jepsen.relate [util :as util]])
  (:import (java.lang ProcessBuilder)))

(defn run
  "Runs `docker args`, returning trimmed stdout. Throws on a nonzero exit
  with the args and stderr in the exception — a failed lifecycle step is
  a setup fault, never something to paper over."
  [& args]
  (let [{:keys [exit out err]} (apply sh "docker" (map str args))]
    (if (zero? exit)
      (str/trim out)
      (throw (ex-info (str "docker " (str/join " " args)
                           " failed (exit " exit "): " (str/trim err))
                      {:args (vec args) :exit exit :err err})))))

(defn try-run
  "Like `run`, but returns nil instead of throwing on a nonzero exit —
  for the idempotent pre-steps only (killing an already-stopped
  container). Never use for an action whose success the run depends on."
  [& args]
  (let [{:keys [exit out]} (apply sh "docker" (map str args))]
    (when (zero? exit) (str/trim out))))

(defn run-result
  "Runs `timeout-secs docker args` to completion (or to the timeout) and
  returns {:exit :out :err} — for probes that assert ON the outcome,
  including on failure or on a hang. run/try-run are the lifecycle
  actions; this is the observation that may legitimately fail."
  [timeout-secs & args]
  (let [{:keys [exit out err]}
        (apply sh "timeout" (str timeout-secs) "docker" (map str args))]
    {:exit exit :out (str/trim (str out)) :err (str/trim (str err))}))

(defn env-of
  "The container's environment as a {name value} map, from docker
  inspect — the exact configured values, compared for equality by both
  setup guards (db.clj's lane config guard and its checkpoint-interval
  assertion); nothing here is interpreted. Throws on a non-VAR=VALUE
  line: docker normalizes env entries, so a malformed one is container
  corruption, never a value to skip silently."
  [container]
  (->> (run "inspect" "-f" "{{range .Config.Env}}{{println .}}{{end}}" container)
       str/split-lines
       (map (fn [line]
              (let [[name value] (str/split line #"=" 2)]
                (if (and name (not= "" name) value)
                  [name value]
                  (throw (ex-info (str "malformed env entry on " container ": "
                                       (pr-str line))
                                  {:container container :line line}))))))
       (into {})))

(defn cmd-of
  "The container's command — the argv its entrypoint runs — as a vector
  of strings, from docker inspect."
  [container]
  (->> (run "inspect" "-f" "{{range .Config.Cmd}}{{println .}}{{end}}" container)
       str/split-lines
       vec))

(defn running?
  "Is the container up? Client-side evidence that a fault actually fired
  (read from docker inspect, not from anything the DB node reports)."
  [container]
  (= "true" (run "inspect" "-f" "{{.State.Running}}" container)))

(defn exit-code
  "The container's most recent exit code — 137 is the SIGKILL signature
  the kill nemesis must leave behind, 0 the clean SIGTERM shutdown's."
  [container]
  (let [raw (run "inspect" "-f" "{{.State.ExitCode}}" container)]
    (or (parse-long raw)
        (throw (ex-info (str "malformed exit code on " container ": "
                             (pr-str raw))
                        {:container container :raw raw})))))

(defn paused?
  "Is the container cgroup-frozen? docker pause freezes every process in
  the container (SIGSTOP-class); State.Running stays TRUE while frozen —
  only State.Paused distinguishes a paused container from a live one.
  Client-side fault evidence for the pause nemesis."
  [container]
  (= "true" (run "inspect" "-f" "{{.State.Paused}}" container)))

(defn exited-within?
  "Polls until the container is no longer running, or `timeout-secs`
  pass. true = it exited inside the budget. `docker kill --signal` only
  DELIVERS the signal — the process exits on its own time — so the
  SIGTERM nemesis polls: a serve that ignored the signal stays up, the
  deadline expires, and the fault evidence says :fired? false instead of
  trusting the signal delivery."
  [container timeout-secs]
  (util/poll-until #(not (running? container)) timeout-secs 250))

(defn logs
  "The container's combined stdout+stderr — MERGED, in write order.
  docker logs maps the container's streams to its own (stdout→stdout,
  stderr→stderr), and serve prints everything forensically relevant to
  STDERR — the startup banner, storage-lock errors, the 'serve: shutting
  down' line, internal-error stderr (ADR-0260: internal errors are never
  returned over the wire) — so a stdout-only capture silently loses it
  all (observed: the lock probe's rejection line read as empty logs).
  clojure's sh captures stdout only, hence the ProcessBuilder with
  redirectErrorStream. Throws on a nonzero exit, like `run`: a failed
  read is never an empty log — an empty string would pass for a healthy
  container's (and for an empty SIGTERM baseline)."
  [container]
  (let [args ["docker" "logs" "--tail" "10000" (str container)]
        pb   (-> (ProcessBuilder. ^java.util.List args)
                 (.redirectErrorStream true))
        p    (.start pb)
        out  (with-open [in (.getInputStream p)] (slurp in))
        exit (.waitFor p)]
    (if (zero? exit)
      (str/trim out)
      (throw (ex-info (str "docker logs " container " failed (exit " exit
                           "): " (str/trim out))
                      {:args args :exit exit :out out})))))

(defn logs-record
  "The container's logs for a forensic file (teardown's db-container.log
  and the like): `logs`, or — when docker cannot read them — the failure
  itself, spelled loudly, so the record never reads as an empty healthy
  log and a failed read never aborts teardown."
  [container]
  (try (logs container)
       (catch Throwable t
         (str "LOGS UNAVAILABLE: " (.getMessage ^Throwable t)))))
