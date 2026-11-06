(ns jepsen.relate.db
  "The DB under test: the stock product image, as compose runs it.

  docker-compose.yml pairs this control container with the DB node —
  the relate-bolt image `make docker` builds (scratch, entrypoint
  PID 1), running `relate serve` on /data (a persistent volume),
  preset auth, `--bolt-identity Neo4j/5.26.0` (LABS-649: driver 5.x
  refuses any agent not starting Neo4j/), and (the image's entrypoint
  default) HTTP Query API v2 on 7474 — the entrypoint appends the
  unset surface ports, so the http lane needs no compose change.

  Compose creates and starts the container; this DB layer makes each
  Jepsen run hermetic: setup! verifies the container actually runs the
  lane's config (durability mode, store path, checkpoint interval — and
  that the image's binary really reads RELATE_DURABILITY_MODE,
  ADR-0470), kills it, wipes the data volume with a throwaway busybox
  container (a TRUSTED wipe — the volume must exist and be verifiably
  empty after), asserts the schedule's premise env where one is
  required (mid-checkpoint's checkpoint interval), restarts it, gates
  on the surface's liveness probe (opts :impl — the per-surface wire,
  surface-impl's map, MANDATORY: a missing one would silently gate any
  lane's recovery on the Bolt wire) with the pinned identity before any
  client connects, and — for workloads that need it — places their
  setup writes (the register node, the counter/bank pools) on the
  freshly wiped store, which is what makes the setup's uniqueness
  guarantees sound.
  teardown! kills it again (crash posture — 'SIGKILL leaves a
  recoverable store' is the story under test) and archives
  `docker logs` into the run's store directory: the scratch image has
  no shell and no jepsen.control channel, so docker logs is the only
  forensic output there is."
  (:require [clojure.string :as string]
            [clojure.tools.logging :refer [info]]
            [jepsen [db :as db]
                    [store :as store]]
            [jepsen.relate [client :as c]
                               [docker :as docker]
                               [http-client :as hc]
                               [workload :as wl]]))

(def ^:const setup-timeout-secs 60)

(def ^:private lane-db-config
  "The container config each lane must run with, per store rig — the one
  table the setup guard asserts. The rig (opts :store) is the DB
  packaging the lane runs on:

  - :volume  the stock compose topology (docker-compose.yml), store on
    the persistent data volume. everysecond and snapshot change only
    the durability mode env; the memory-vfs control KEEPS full mode and
    swaps the store to :memory: — the VFS (a store with no durable
    medium) is the variable it isolates, so full durability on it is
    the known-bad config the control must fail.
  - :lazyfs  the power-off rig (docker-compose.lazyfs.yml, LABS-654),
    store on the lazyfs FUSE mount. full is the certified half;
    everysecond and snapshot are the non-syncing controls
    run-poweroff.sh must see lose acked writes. No memory-vfs row: a
    :memory: store on a power-volatile disk isolates nothing.

  Each rig carries its :lanes rows (a subset of the lane vocabulary
  relate.clj's --lane validates — a lane with no row on its rig is
  a loud setup fault) and its :recreate remedy, the guard's hint for a
  container that does not match."
  {:volume {:recreate (str "LANE=<lane> ./run.sh (run.sh exports "
                           "RELATE_DURABILITY_MODE / RELATE_DATABASE / "
                           "RELATE_CHECKPOINT_INTERVAL_SECS to compose)")
            :lanes {"full"        {:durability-mode "full"        :database "/data/graph"}
                    "everysecond" {:durability-mode "everysecond" :database "/data/graph"}
                    "snapshot"    {:durability-mode "snapshot"    :database "/data/graph"}
                    "memory-vfs"  {:durability-mode "full"        :database ":memory:"}}}
   :lazyfs {:recreate (str "./run-poweroff.sh (it exports "
                           "RELATE_DURABILITY_MODE / "
                           "RELATE_CHECKPOINT_INTERVAL_SECS to compose and "
                           "force-recreates the DB node per half)")
            :lanes {"full"        {:durability-mode "full"        :database "/mnt/lazyfs/graph"}
                    "everysecond" {:durability-mode "everysecond" :database "/mnt/lazyfs/graph"}
                    "snapshot"    {:durability-mode "snapshot"    :database "/mnt/lazyfs/graph"}}}})

(def ^:private wiring-probe-mode
  "A deliberately invalid durability mode for the image wiring probe:
  ADR-0470's fail-open contract says a binary that reads the variable
  refuses a bad value loudly before the store opens, and a binary that
  does not (a stale image) starts serving instead — the probe's timeout
  catches the hang."
  "durability-mode-wiring-probe")

(defn- cmd-value
  "The value after `flag` in an argv, or nil when the flag is absent or
  valueless."
  [argv flag]
  (loop [xs (seq argv)]
    (when xs
      (if (= flag (first xs))
        (second xs)
        (recur (next xs))))))

(defn- verify-container-config!
  "Asserts the DB container runs THIS lane's config on THIS rig — the
  silent-no-op guard for the compose interpolation that passes the lane
  in. A lane whose mode env never reached the container (a typo'd
  variable, a stale container from an older compose command) would
  silently test full durability and read green; likewise a
  mid-checkpoint lane whose RELATE_CHECKPOINT_INTERVAL_SECS=5 never
  reached the container would degenerate into a kill9 rerun at the
  default 300s interval, certifying nothing about mid-rotate crashes.
  docker inspect is the source, exact equality is the comparison (an
  interpolated-but-empty variable never passes for a set one), and a
  mismatch — or a lane with no row on its rig — is a loud setup fault."
  [{:keys [db-container store lane checkpoint-interval-secs]}]
  (let [row (get-in lane-db-config [store :lanes lane])
        _   (when-not row
              (throw (ex-info (str "lane " (pr-str lane) " has no container "
                                   "config on store rig " (pr-str store)
                                   " (rigs and lanes: "
                                   (pr-str (into {} (for [[rig {:keys [lanes]}] lane-db-config]
                                                      [rig (sort (keys lanes))])))
                                   ")")
                              {:store store :lane lane})))
        env      (docker/env-of db-container)
        expected (assoc row :checkpoint-interval-secs (str checkpoint-interval-secs))
        actual   {:durability-mode (get env "RELATE_DURABILITY_MODE")
                  :database        (cmd-value (docker/cmd-of db-container)
                                              "--database")
                  :checkpoint-interval-secs (get env
                                                 "RELATE_CHECKPOINT_INTERVAL_SECS")}]
    (when-not (= expected actual)
      (throw (ex-info (str "db container " db-container " does not run lane "
                           (pr-str lane) " config on store rig " (pr-str store)
                           ": expected " (pr-str expected)
                           ", got " (pr-str actual)
                           " — recreate it with the lane's environment: "
                           (get-in lane-db-config [store :recreate]))
                      {:store store :lane lane
                       :expected expected :actual actual})))))

(defn- verify-mode-wiring!
  "Proves the DB image's binary actually resolves
  RELATE_DURABILITY_MODE (ADR-0470): runs a throwaway container from
  the same image with an invalid value and asserts the loud refusal.
  Without this, a stale product image — built before ADR-0470 — would
  silently ignore the lane's mode env: everysecond and snapshot would
  run full durability and their bound checks would pass vacuously.

  The probe names the product supervisor (/bin/entrypoint) as its
  entrypoint: the stock image's own ENTRYPOINT, and the binary the
  lazyfs rig's entrypoint execs after mounting — so the probe asks the
  same binary on both rigs without needing the rig's privileged FUSE
  mount (which an unprivileged probe container cannot make)."
  [{:keys [db-container]}]
  (let [image (docker/run "inspect" "-f" "{{.Config.Image}}" db-container)
        {:keys [exit out err]}
        (docker/run-result 30 "run" "--rm"
                           "--entrypoint" "/bin/entrypoint"
                           "-e" (str "RELATE_DURABILITY_MODE="
                                     wiring-probe-mode)
                           image
                           "--database" "/wiring-probe" "--bolt" "7687")
        output (str out "\n" err)]
    (when (or (zero? exit)
              (= 124 exit)
              (not (string/includes? output "RELATE_DURABILITY_MODE")))
      (throw (ex-info (str "product image " image " ignores "
                           "RELATE_DURABILITY_MODE (probe exit " exit
                           ", output: " (pr-str output) ") — it predates the "
                           "ADR-0470 wiring, so no lane can trust its mode. "
                           "Rebuild it from this repo (make docker, or "
                           "docker build -t <tag> .) and point "
                           "RELATE_IMAGE at the rebuild")
                      {:image image :exit exit :output output})))))
(defn- workload-setup!
  "Runs the workload's Bolt-level setup (nil for the set lane, which
  creates its nodes op by op) after the DB is up and identity-pinned —
  once per run, before any client op, on the wiped store."
  [{:keys [workload-setup bolt-uri auth-user auth-password]}]
  (when workload-setup
    (workload-setup bolt-uri auth-user auth-password)))

(defn- stop!
  "SIGKILL the container if it is up (idempotent — `try-run` because an
  already-stopped container is exactly the expected precondition)."
  [{:keys [db-container]}]
  (when (docker/running? db-container)
    (docker/run "kill" db-container)))

(defn- volume-mount
  "The -v flag pairing `data-volume` with /data."
  [data-volume]
  (str data-volume ":/data"))

(defn- wipe!
  "Empties the data volume with a throwaway busybox container, so every
  run starts from an empty store — /data/* plus the dotfiles serve
  writes beside the store directory (.auth-token, .cypherlite.writer).
  The DB container must be down.

  The wipe is TRUSTED, not exit-code-trusted: `docker run -v NAME:...`
  silently auto-creates a missing volume name as a fresh empty one, so a
  name mismatch (compose's volume vs this harness's --data-volume) wiped
  the WRONG volume and still exited 0 — a silent no-op reading green,
  the exact failure mode docker.clj's docstring warns about. Two guards
  make a silent no-op impossible: the volume must already exist, and
  /data must verifiably be empty afterwards. Either failing is a setup
  fault, never something to paper over."
  [{:keys [data-volume]}]
  (docker/run "volume" "inspect" data-volume)
  (docker/run "run" "--rm" "-v" (volume-mount data-volume) "busybox"
              "sh" "-c" "rm -rf /data/* /data/.[!.]*")
  (let [leftover (docker/run "run" "--rm" "-v" (volume-mount data-volume)
                             "busybox" "sh" "-c" "ls -A /data")]
    (when-not (= "" leftover)
      (throw (ex-info (str "data volume wipe left residue — volume "
                           data-volume " still holds " (pr-str leftover))
                      {:data-volume data-volume :leftover leftover})))))

(defn surface-impl
  "The per-surface wire, ONE dispatch table — the single place a lane
  plugs in: the client constructor every lane builds its client through
  — (client uri user password handlers), where `handlers` is the
  workload's Bolt op-handler map (the set lanes pass
  set/client-handlers, the Elle lanes their workload's); the http
  client speaks the Query API wire with its own set/register handlers
  and takes no handler map, so a non-nil one there is a dispatch bug
  and throws — the liveness probe and restart-liveness wait the DB
  layer and nemesis verify faults through, and (when the surface's
  workload needs a setup write) the register initializer. Lives here,
  the lowest module every :impl consumer (the lanes in
  jepsen.relate, the compare workload's db) already requires.
  `surface` is :bolt or :http. `:takes-handlers?` states whether the
  surface's client constructor takes the Bolt handler map — the one fact
  a lane consults instead of re-deriving it from the surface keyword."
  [surface]
  (case surface
    :bolt {:takes-handlers? true
           :client  (fn [uri user password handlers]
                      (c/->BoltClient nil uri user password handlers))
           :probe   c/probe!
           :wait-up c/wait-up!}
    :http {:takes-handlers? false
           :client        (fn [uri user password handlers]
                            (when (some? handlers)
                              (throw (ex-info (str "the http surface carries its "
                                                   "own op handlers; got a Bolt "
                                                   "handler map")
                                              {:handlers (keys handlers)})))
                            (hc/->QueryApiClient nil uri user password))
           :probe         hc/probe!
           :wait-up       hc/wait-up!
           :init-register! hc/init-register!}
    (throw (ex-info (str "unknown surface " (pr-str surface)
                         " — must be :bolt or :http")
                    {:surface surface}))))

(defn docker-db
  "A jepsen.db/DB over the compose-run DB container. `opts` wants
  :db-container, :data-volume, :uri, :auth-user, :auth-password, the
  lane it must verify (:store — the rig, a lane-db-config key — :lane
  plus, from the schedule row, :checkpoint-interval-secs), :impl — the per-surface wire
  (surface-impl's map: wait-up, optionally the register setup write),
  MANDATORY — and/or :workload-setup (the workload's own Bolt-level
  setup, e.g. the counter/bank pools), resolved via surface-impl."
  [opts]
  (reify db/DB
    (setup! [_this _test _node]
      (verify-mode-wiring! opts)
      (verify-container-config! opts)
      (stop! opts)
      (wipe! opts)
      (docker/run "start" (:db-container opts))
      ;; Gate on liveness + pinned identity before any client connects —
      ;; through the surface's own wait-up. :impl is mandatory: a
      ;; fallback here would gate an http lane's recovery on the Bolt
      ;; wire.
      (let [impl    (:impl opts)
            wait-up (:wait-up impl)
            _       (when-not (fn? wait-up)
                      (throw (ex-info (str "docker-db's opts carry no :wait-up"
                                           " — pass surface-impl's map as :impl;"
                                           " the liveness gate has no fallback")
                                      {:impl impl})))
            identity (wait-up (:uri opts)
                              (:auth-user opts) (:auth-password opts)
                              setup-timeout-secs)]
        (info "db up:" (:db-container opts) "identity:" identity
              "lane:" (:lane opts))
        ;; The register workload's setup write, when this surface runs
        ;; it: placed once, on the empty store, before any client can
        ;; MERGE concurrently (workload.clj).
        (when-let [init-register! (:init-register! impl)]
          (init-register! (:uri opts) (:auth-user opts) (:auth-password opts))
          (info "register initialized with value" wl/register-initial-value))
        ;; The workload's own Bolt-level setup (the counter lane's
        ;; :Counter pool, the bank lane's :Account pool) — on the
        ;; freshly wiped store, which is what makes the setup's
        ;; uniqueness guarantees sound.
        (workload-setup! opts)
        (info "workload setup done")))

    (teardown! [_this test _node]
      (stop! opts)
      ;; docker logs is the only forensic channel a scratch image has;
      ;; keep it with the run's history (stderr merged — serve prints
      ;; its forensics there).
      (spit (store/path! test "db-container.log")
            (str (docker/logs-record (:db-container opts)) "\n")))))
