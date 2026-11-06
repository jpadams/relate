(ns jepsen.relate.schedule
  "The nemesis schedule table: one row per fault schedule (LABS-650's
  kill lane, LABS-651's widening), the single home for what each
  schedule's fault cycle contains. The generator (which ops, in what
  order), the nemesis (the schedule's knobs — kill jitter, checkpoint
  sampling, the storage-lock probe), the DB setup guard (the env the
  container must carry), and the fault-evidence checker (which op fs
  must have been exercised, which extra pins must hold) all consult this
  table — never a re-derived copy at a use site.

  A row states:

  :name                      the test name (jepsen store directory, log
                             naming)
  :cycle                     the fault cycle's op fs IN ORDER — the
                             generator interleaves a
                             `--nemesis-interval` sleep before each;
                             each f is a nemesis invoke (a fault or a
                             recovery). The LAST f is always the
                             recovery, so a run cut anywhere by the time
                             limit still ends with the DB up for the
                             final read.
  :kill-jitter-secs          random pre-kill delay ceiling: kills spread
                             uniformly over the checkpoint period
                             instead of phase-locking to the timer
  :checkpoint-evidence?      sample the manifest's checkpoint LSN around
                             each fault; the checker requires rotation
                             in every up-phase
  :checkpoint-interval-secs  the RELATE_CHECKPOINT_INTERVAL_SECS the
                             DB container must actually carry (compose
                             interpolates it; setup! asserts it)
  :storage-lock-probe?       while the DB is paused, a second serve
                             against the same /data store must be
                             rejected with the storage-locked error
                             (ADR-0097) — the flock pin"
  (:require [clojure.string :as str]))

(def schedules
  {:kill9
   {:name "relate-set-kill9"
    :cycle [:start :stop]
    :doc "docker kill (SIGKILL to PID 1, no final checkpoint) / docker
          start (WAL replay) — the v1 lane (LABS-650)."}

   :pause
   {:name "relate-set-pause"
    :cycle [:pause :resume]
    :storage-lock-probe? true
    :doc "docker pause / docker unpause — the cgroup freezer freezes
          every process in the container: the serve process stays alive,
          mid-write, still holding the .cypherlite.writer flock (the
          freezer does not release file locks). While frozen, a second
          serve against the same store must be rejected with the
          storage-locked error — the flock pin."}

   :sigterm-kill9
   {:name "relate-set-sigterm-kill9"
    :cycle [:sigterm :stop :start :stop]
    :doc "clean-to-crash: SIGTERM (serve's clean shutdown + final
          checkpoint, exit 0), restart, then kill -9 (no final
          checkpoint), restart — the clean path and the crash path
          interleaved; nothing acknowledged may be lost by either."}

   :mid-checkpoint
   {:name "relate-set-mid-checkpoint"
    :cycle [:start :stop]
    :kill-jitter-secs 5
    :checkpoint-evidence? true
    :checkpoint-interval-secs 5
    :doc "kill -9 against a serve checkpointing every 5s
          (RELATE_CHECKPOINT_INTERVAL_SECS=5): kills jittered
          uniformly over the checkpoint period land at every phase of
          the rotate/dump, and the manifest's checkpoint LSN must
          advance in every up-phase — the timer, not the kill (SIGKILL
          takes no final checkpoint), rotated the store."}})

(def evidence-keys
  "Every nemesis op :f a fault cycle can issue, mapped to the evidence
  key its completion must carry: a FAULT (:start the crash, :sigterm,
  :pause) proves itself with :fired?, a RECOVERY (:stop the restart,
  :resume the thaw) with :recovered?. The one fault/recovery
  classification — the fault-evidence checker dispatches on it, never on
  which key an op value happens to carry."
  {:start   :fired?
   :sigterm :fired?
   :pause   :fired?
   :stop    :recovered?
   :resume  :recovered?})

(defn row
  "The schedule row for a --nemesis value. The row is the one canonical
  schedule shape; callers never restate it. Throws naming the valid
  schedules when the key is unknown — a typo'd schedule must never
  silently run some other lane."
  [k]
  (or (get schedules k)
      (throw (ex-info (str "unknown nemesis schedule " (pr-str k)
                           "; valid: "
                           (str/join ", " (map name (sort (keys schedules)))))
                      {:schedule k :valid (keys schedules)}))))
