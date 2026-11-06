(ns jepsen.relate.checker
  "The set-workload checkers, one per durability lane (the LABS-653
  crash-recovery matrix).

  The FULL lane's checker compiles the ADR-0077 crash-durability
  contract into the op taxonomy the Bolt spec hands us (ack = COMMIT
  SUCCESS, a received FAILURE = definitively not committed, a
  connection error = uncertain):

    acked     ⊆ recovered   every acknowledged write survived the crashes
    recovered ⊆ attempted   no ghosts: only ids the harness attempted
    failed ∩ recovered = ∅   a definitively-rejected write must not have
                             committed

  `attempted` spans every :add the generator issued — :ok, :fail, and
  :info alike — because an :info op (the wire died mid-commit) may or
  may not have committed; both outcomes are legal for it, and only
  ghosts and lost acks are findings.

  The RELAXED lanes (everysecond, snapshot) assert the same contract
  with ADR-0077's loss bound instead of its first clause: an
  acknowledged write MAY be lost to a crash, but only one acknowledged
  within the mode's loss window before the kill — one second for
  everysecond's background barrier, one checkpoint interval for
  snapshot's checkpoint cut. The BOUND is the assertion, never the
  absence of loss: a run with zero loss passes, and so does a run that
  loses exactly the in-window acks. Ghosts and rejected-but-present
  writes stay findings in every mode — relaxation never fabricates
  data.

  The MEMORY-VFS control inverts the full contract on purpose: it is
  valid only when acknowledged writes ARE lost — the calibration that
  the kill fault can actually fail a known-bad config (a store with no
  durable medium), without which every green lane above it is vacuous.

  The fault-evidence checker fails the run unless the requested faults
  actually happened — every op :f the fault cycle issues was
  exercised, every fault FIRED (its own recorded evidence: container
  state, exit code, the lane's wire-level probe), every recovery
  RECOVERED (reachable again with the pinned identity) — plus the
  cycle's own pins (the pause schedule's storage-lock probe, the
  mid-checkpoint schedule's checkpoint rotation, the poweroff lane's
  clear-cache delivery). The guard against the silent-no-op fault
  machinery class of bug (jepsen 0.3.12-0.3.14's grepkill!): a green
  run whose faults never happened certifies nothing.

  The client-hygiene checker is the same philosophy on the client side
  of the Elle lanes (LABS-652): a green run is only evidence if the
  ops themselves were legal. Every completed op carries the
  error-class its exception mapped to (client.clj), and a run fails
  when any completion is :harness class (an invariant guard or an
  unrecognized throw — never a legal workload outcome) or :connection
  class without a nemesis (a dropped connection with no faults in play
  is itself a finding, not uncertainty to wave through)."
  (:require [clojure.set :as set]
            [jepsen [checker :as checker]
                    [history :as h]]
            [jepsen.relate [schedule :as schedule]
                               [workload :as wl]]))

(defn- invocations
  [history f]
  (filter #(and (= f (:f %)) (= :invoke (:type %))) history))

(defn- completions
  [history f]
  (filter #(and (= f (:f %)) (not= :invoke (:type %))) history))

(defn- sample
  "Up to `n` sorted elements — full lists when short, evidence samples
  when long."
  [n coll]
  (->> coll sort (take n) vec))

(defn- set-facts
  "The set workload's final-state facts — the ONE extraction every
  set-lane checker consults: attempted/acked/failed id sets, the final
  read op, and the derived recovered/lost/ghost/zombie sets.
  Acknowledged writes carry their completion :time (history-clock
  nanos) because the relaxed lanes' bounds are temporal.

  A missing read yields empty recovered/lost sets and a nil :read; the
  checkers report that as their own loud error rather than comparing
  against a phantom empty store."
  [history]
  (let [attempted (set (map :value (invocations history :add)))
        acked     (->> (completions history :add)
                       (filter #(= :ok (:type %)))
                       (map (fn [op] {:id (:value op) :acked-at (:time op)}))
                       doall)
        failed    (set (map :value (filter #(= :fail (:type %))
                                           (completions history :add))))
        read      (first (completions history :read))
        recovered (set (:ids (:value read)))]
    {:attempted attempted
     :acked     acked
     :failed    failed
     :read      read
     :recovered recovered
     :lost      (set/difference (set (map :id acked)) recovered)
     :ghosts    (set/difference recovered attempted)
     :zombies   (set/intersection failed recovered)}))

(defn- no-final-read-error
  "The shared error result for a history without a final read. `noun`
  names what the final read would have observed — \"state\" for the
  set lanes, \"register\" for the register workload."
  [noun]
  {:valid? false
   :error (str "history contains no final read — the recovered " noun
               " was never observed")})

(defn- set-report
  "The set-family result map every set-lane checker reports, built from
  set-facts: the attempted/acknowledged/recovered counts, the lost /
  ghost / rejected-but-present sets (count + a 20-element sample), and
  the final read's type. Each checker merges its own verdict keys over
  it."
  [{:keys [read attempted acked recovered lost ghosts zombies]}]
  {:attempted-count      (count attempted)
   :acknowledged-count   (count acked)
   :recovered-count      (count recovered)
   :lost-acked-writes    {:count (count lost) :sample (sample 20 lost)}
   :ghost-ids            {:count (count ghosts) :sample (sample 20 ghosts)}
   :rejected-but-present {:count (count zombies) :sample (sample 20 zombies)}
   :read-type            (:type read)})

(defn set-final-state
  "Final-state set equality over the history — the full-durability
  contract: acked ⊆ recovered ⊆ attempted, plus no fabricated
  survivors of definitively-failed ops. The recovered state is the
  final :read op's value."
  []
  (reify checker/Checker
    (check [_this _test history _opts]
      (let [{:keys [read lost ghosts zombies] :as facts} (set-facts history)]
        (if (nil? read)
          (no-final-read-error "state")
          (assoc (set-report facts)
                 :valid? (and (= :ok (:type read))
                              (empty? lost)
                              (empty? ghosts)
                              (empty? zombies))))))))

(defn- fired-kills
  "The nemesis :start completions whose evidence says the SIGKILL
  landed. Each carries :killed-at — the history-clock instant the
  nemesis captured immediately before docker kill (nemesis.clj); the
  completion op's own :time is post-verification and would smear the
  loss window by the whole verification latency."
  [history]
  (->> history
       (filter #(= :nemesis (:process %)))
       (filter #(= :start (:f %)))
       (filter #(get-in % [:value :fired?]))
       (map :value)))

(defn- kill-evidence-error
  "The error string for fired kills missing their :killed-at instant,
  or nil when the evidence is complete. The bound refuses to run on
  incomplete kill evidence — guessing the instant would quietly widen
  or narrow the window."
  [kills]
  (when-let [missing (seq (remove :killed-at kills))]
    (str "fired kills without a recorded :killed-at instant: "
         (pr-str (mapv #(dissoc % :killed-at) missing)))))

(defn- ack-evidence-error
  "The error string for acknowledged adds missing their completion
  :time, or nil — the bound needs WHEN each write was acknowledged."
  [acked]
  (when-let [missing (seq (remove :acked-at acked))]
    (str "acknowledged adds without completion times: "
         (pr-str (mapv :id missing)))))

(defn- loss-permitted?
  "May the write acknowledged at `acked-at` have been lost to a kill at
  one of `kill-times`? Only when a fired kill lands INSIDE the loss
  window after the ack: the mode's barrier — everysecond's background
  worker, snapshot's checkpoint — covers the write once the window
  elapses, so a kill after the window must not lose it. A kill before
  the ack says nothing about this write."
  [kill-times window-nanos acked-at]
  (boolean (some (fn [kill-at]
                   (and (<= acked-at kill-at)
                        (<= kill-at (+ acked-at window-nanos))))
                 kill-times)))

(defn durability-bound
  "The ADR-0077 relaxed-mode contract over the same final-state sets:
  acknowledged writes may be lost to the crashes, but ONLY those
  acknowledged within `window-secs` seconds before a fired kill.
  `window-secs` is the mode's loss window — one second for
  everysecond, the checkpoint interval for snapshot. Zero loss passes;
  in-window loss passes; an older acknowledged write going missing is
  the finding. Ghosts and rejected-but-present writes are findings in
  every mode."
  [window-secs]
  (reify checker/Checker
    (check [_this _test history _opts]
      (let [{:keys [read acked lost ghosts zombies] :as facts}
            (set-facts history)
            kills       (fired-kills history)
            kill-error  (kill-evidence-error kills)
            ack-error   (ack-evidence-error acked)]
        (cond
          (nil? read)     (no-final-read-error "state")
          kill-error      {:valid? false :error kill-error}
          ack-error       {:valid? false :error ack-error}
          :else
          (let [kill-times  (mapv :killed-at kills)
                window-nanos (long (* window-secs 1e9))
                lost-ops     (filter #(contains? lost (:id %)) acked)
                violations   (remove #(loss-permitted? kill-times
                                                      window-nanos
                                                      (:acked-at %))
                                     lost-ops)]
            (assoc (set-report facts)
                   :valid?      (and (= :ok (:type read))
                                     (empty? violations)
                                     (empty? ghosts)
                                     (empty? zombies))
                   :window-secs window-secs
                   :kills-fired (count kill-times)
                   :lost-outside-loss-window
                   {:count  (count violations)
                    :sample (sample 20 (map :id violations))})))))))

(defn memory-vfs-control
  "The negative control for the whole lane program: the same set
  workload and kill fault against a Memory VFS store — full durability
  mode on a store with no durable medium — must LOSE acknowledged
  writes. Valid only when the full-mode invariant fails exactly that
  way (acked ⊄ recovered by at least one lost ack); ghosts and
  rejected-but-present remain findings. A control that loses nothing
  means the fault cannot bite, and every green lane above it is
  vacuous — a fault that cannot fail a known-bad config cannot
  validate a good one."
  []
  (reify checker/Checker
    (check [_this _test history _opts]
      (let [{:keys [read lost ghosts zombies] :as facts} (set-facts history)]
        (if (nil? read)
          (no-final-read-error "state")
          (assoc (set-report facts)
                 :valid? (and (= :ok (:type read))
                              (pos? (count lost))
                              (empty? ghosts)
                              (empty? zombies))
                 :error  (when (empty? lost)
                           (str "negative control lost no acknowledged "
                                "writes — the kill fault did not bite; "
                                "every green lane above it is vacuous"))
                 :full-mode-invariant-held? (empty? lost)))))))

(defn register-final-state
  "Final-state register persistence over the history: the register's
  final value must be a value the harness attempted — an acknowledged
  write, an uncertain (:info) write, or the initial value the setup
  write placed — and never a definitively-rejected write or a value
  nobody wrote (a ghost). The recovered value is the final :read op's
  :register.

  Last-write-wins is why 'acked ⊆ recovered' has no register analog: a
  lawful later write overwrites any acked one, so the durable contract
  the register can check is exactly 'some plausible writer wrote the
  final value' plus no-ghosts — the write boundary itself is what the
  set lane checks. The checker reports WHICH kind of write the final
  value came from; an :info write winning is legal (uncertain means
  both outcomes), an initial value means no write survived, and a
  vanished register is a lost acked setup write."
  []
  (reify checker/Checker
    (check [_this _test history _opts]
      (let [attempted (set (map :value (invocations history :write)))
            acked     (set (map :value (filter #(= :ok (:type %))
                                               (completions history :write))))
            failed    (set (map :value (filter #(= :fail (:type %))
                                               (completions history :write))))
            read      (first (completions history :read))]
        (if (nil? read)
          (no-final-read-error "register")
          (let [recovered (:register (:value read))
                initial   wl/register-initial-value]
            (merge
             {:attempted-count    (count attempted)
              :acknowledged-count (count acked)}
             (cond
               (nil? recovered)
               {:valid? false
                :error (str "register node missing at the final read — the "
                            "acked setup write (initial value " initial
                            ") was lost")}

               (contains? failed recovered)
               {:valid? false
                :error (str "register holds a definitively-rejected write: "
                            recovered)
                :register-value recovered}

               (not (or (= initial recovered)
                        (contains? attempted recovered)))
               {:valid? false
                :error (str "register ghost: value " recovered
                            " was never written")
                :register-value recovered}

               :else
               {:valid? true
                :register-value recovered
                :from-acked-write? (contains? acked recovered)
                :from-initial? (= initial recovered)
                :from-uncertain-write? (and (not (contains? acked recovered))
                                            (not= initial recovered))}))))))))

(defn- nemesis-ops
  "The completed nemesis ops carrying evidence, in history order."
  [history]
  (->> history
       (filter #(= :nemesis (:process %)))
       (filter :value)))

(defn- unproved-ops
  "Every fault that did not fire and every recovery that did not come
  back — the ops whose evidence key reads false. Which key an op must
  carry is dispatched on its :f through schedule/evidence-keys (faults
  :fired?, recoveries :recovered?), never sniffed from the value. An op
  whose :f has no evidence key, or whose value lacks the key its :f
  names, is broken evidence machinery: it fails too (unkeyed), never
  silently passes."
  [ops]
  (let [proved? (fn [op] (get (:value op) (schedule/evidence-keys (:f op))))
        keyed?  (fn [op] (let [k (schedule/evidence-keys (:f op))]
                           (and k (contains? (:value op) k))))]
    {:unfired (remove proved? ops)
     :unkeyed (remove keyed? ops)}))

(defn- storage-lock-rejections
  "The pause schedule's flock pin: every pause op must carry proof that
  a second serve against the frozen store was rejected with the
  storage-locked error (ADR-0097). A pause without the probe, or a
  probe that was NOT rejected, is returned as a failure."
  [ops]
  (->> ops
       (filter #(= :pause (:f %)))
       (remove #(get-in % [:value :storage-locked?]))
       vec))

(defn- checkpoint-rotations
  "The mid-checkpoint schedule's rotation evidence, from the LSN samples
  the nemesis recorded: walk the kill/restart ops in history order and
  pair each restart with the NEXT kill. Every pair must show the
  checkpoint LSN strictly ADVANCING between the restart's post-replay
  sample and the kill's pre-kill sample — under SIGKILL there is no
  final checkpoint and none at open, so the 5s timer is the only
  possible writer: this is the proof the container really was
  checkpointing frequently, and that kills landed on a store whose
  checkpoints were actively rotating.

  A missing sample where one is required (any restart, or any kill that
  follows a restart) is a machinery fault — the run fails rather than
  guessing. The run's FIRST kill precedes any restart and is exempt: a
  store with no checkpoint yet has no manifest to read."
  [ops]
  (let [steps (->> ops
                   (filter #(#{:start :stop} (:f %)))
                   (map (fn [op] [(:f op)
                                  (get-in op [:value
                                              (case (:f op)
                                                :start :pre-kill-lsn
                                                :stop  :post-restart-lsn)])])))
        {:keys [pairs broken]}
        (reduce (fn [{:keys [pairs pending broken]} [f lsn]]
                  (case f
                    :stop  {:pairs   pairs
                            :pending {:restart-lsn lsn}
                            :broken  (if (nil? lsn)
                                       (conj broken {:missing :post-restart-lsn})
                                       broken)}
                    :start (if (nil? pending)
                             ;; the run's first kill: no restart before it
                             {:pairs pairs :pending pending :broken broken}
                             (let [pair {:restart-lsn (:restart-lsn pending)
                                         :kill-lsn    lsn}]
                               {:pairs   (conj pairs pair)
                                :pending nil
                                :broken  (if (nil? lsn)
                                           (conj broken {:missing :pre-kill-lsn})
                                           broken)}))))
                {:pairs [] :pending nil :broken []}
                steps)
        rotated?  (fn [p] (and (number? (:restart-lsn p))
                               (number? (:kill-lsn p))
                               (> (:kill-lsn p) (:restart-lsn p))))
        stalled   (remove rotated? pairs)]
    {:valid?       (and (seq pairs) (empty? broken) (empty? stalled))
     :pairs        pairs
     :rotated      (count (filter rotated? pairs))
     :stalled      (vec stalled)
     :broken       (vec broken)}))

(defn fault-evidence
  "Fails the run unless the requested faults actually happened — the
  guard against the silent-no-op fault machinery class of bug (jepsen
  0.3.12-0.3.14's grepkill!): a green run whose faults never happened
  certifies nothing. `faults` is the fault spec the dispatcher resolved
  (jepsen.relate/fault-plan): a schedule table's row
  (schedule.clj), the poweroff lane's spec, or the Elle lanes' kill
  cycle. The contract, over the evidence the nemesis records on each
  op:

  - every op :f the spec's :cycle issues was exercised at least once
    (a cycle that never ran means the time limit cut before any fault
    — nothing was certified);
  - every fault FIRED and every recovery RECOVERED — the ops whose
    evidence key (schedule/evidence-keys, by :f: :fired? for faults,
    :recovered? for recoveries) reads false fail the run, and an op
    value missing the key its :f names is broken evidence machinery and
    fails too (unkeyed), never silently passes;
  - :storage-lock-probe? (the pause schedule) — every pause rejected a
    second serve against the frozen store with the storage-locked
    error (the flock pin, ADR-0097);
  - :checkpoint-evidence? (the mid-checkpoint schedule) — the
    checkpoint LSN advanced between every restart and the next kill
    (the 5s timer really rotated the store);
  - :cache-clear-required? (the poweroff lane) — every crash's
    clear-cache fault was delivered first: a power-off without the
    cache clear is a plain kill and certifies nothing about un-fsynced
    bytes.

  The 0-arity (and an omitted :cycle) is the kill cycle [:start :stop]
  — the Elle toggle's."
  ([] (fault-evidence {}))
  ([{:keys [cycle cache-clear-required? storage-lock-probe?
            checkpoint-evidence?]
     :or {cycle [:start :stop]}}]
   (reify checker/Checker
     (check [_this _test history _opts]
       (let [ops               (nemesis-ops history)
             {:keys [unfired unkeyed]} (unproved-ops ops)
             missing-fs        (remove (fn [f] (some #(= f (:f %)) ops))
                                       cycle)
             kills             (filter #(= :start (:f %)) ops)
             cache-missing     (when cache-clear-required?
                                 (vec (remove #(get-in % [:value :cache-cleared?])
                                              kills)))
             lock-failures     (when storage-lock-probe?
                                 (storage-lock-rejections ops))
             rotation          (when checkpoint-evidence?
                                 (checkpoint-rotations ops))]
         {:valid? (and (empty? missing-fs)
                       (empty? unfired)
                       (empty? unkeyed)
                       (empty? cache-missing)
                       (empty? lock-failures)
                       (or (nil? rotation) (:valid? rotation)))
          :nemesis-ops (count ops)
          :missing-fs (vec missing-fs)
          :fault-never-fired? (seq missing-fs)
          :faults-without-effect (mapv :value unfired)
          :ops-without-evidence-key (mapv :value unkeyed)
          :kills-fired (count kills)
          :cache-clears-delivered (count (filter #(get-in % [:value :cache-cleared?])
                                                 kills))
          :cache-clears-missing (mapv :value cache-missing)
          :restarts-recovered (count (filter #(= :stop (:f %)) ops))
          :storage-lock-rejected (count (filter #(and (= :pause (:f %))
                                                      (get-in % [:value :storage-locked?]))
                                                ops))
          :storage-lock-failures (mapv :value lock-failures)
          :checkpoint-rotation rotation})))))

(defn client-hygiene
  "Fails the run when any client completion carries a harness-class
  error (an invariant-guard throw or an unrecognized client exception —
  never a legal outcome of the workload), or a connection-class error
  when no faults were requested (with the kill nemesis in play,
  connection errors are the expected fallout and are tolerated — the
  :info uncertainty is what Elle and the bank checker are built to
  consume).

  This is what makes an all-erroring run red instead of quietly green:
  without it, a workload whose every op died in the client (missing
  setup nodes, a dialect break) could still pass the Elle checks,
  because uncertain ops make no isolation claims."
  [tolerate-connection-errors?]
  (reify checker/Checker
    (check [_this _test history _opts]
      (let [errored (->> history
                         h/client-ops
                         (filter :error-class))
            harness (filter #(= :harness (:error-class %)) errored)
            connection (filter #(= :connection (:error-class %)) errored)
            bad (concat harness (when-not tolerate-connection-errors?
                                  connection))]
        {:valid? (empty? bad)
         :harness-errors (count harness)
         :connection-errors (count connection)
         :connection-errors-tolerated? tolerate-connection-errors?
         :offenses (mapv (fn [op] (select-keys op
                                               [:index :process :f :type
                                                :error :error-class]))
                         (take 20 bad))}))))
