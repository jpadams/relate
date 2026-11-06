(ns jepsen.relate.checker-test
  "Unit contracts for the lane checkers, on synthetic histories in the
  exact op taxonomy the client and nemesis produce:

  - set-final-state: the full-mode invariants (v1 semantics, unchanged)
  - durability-bound: the BOUND is the assertion — in-window loss
    passes, out-of-window loss and unexcused loss fail, zero loss
    passes, and incomplete kill/ack evidence is a loud invalid
  - memory-vfs-control: loss of acked writes is the ONLY valid
    outcome — the control that fails to fail is itself a finding
  - fault-evidence: every op's evidence key is dispatched on its :f
    (schedule/evidence-keys) — a fault's :fired?, a recovery's
    :recovered?; an op missing its key is unkeyed and fails
  - the DB setup guard (db.clj's verify-container-config!), pinned
    against the docker-inspect shape each compose rig renders — the
    power-off lane's lazyfs rig in particular, which no CI gate runs
    live (it needs a FUSE host)"
  (:require [clojure.test :refer [deftest is run-tests testing]]
            [jepsen [checker :as checker]]
            [jepsen.relate [checker :as cc]
                               [db :as cdb]
                               [docker :as docker]
                               [nemesis :as cnem]]))

;;; --- history builders -------------------------------------------------------

(def ^:const sec
  "One second in the history clock's nanoseconds."
  (long 1e9))

(defn- invoke-add
  [id t]
  {:type :invoke :f :add :value id :process 0 :time t})

(defn- ack
  "An acknowledged :add — COMMIT SUCCESS at history time t."
  [id t]
  {:type :ok :f :add :value id :process 0 :time t})

(defn- rejected
  "A definitively-rejected :add."
  [id t]
  {:type :fail :f :add :value id :process 0 :time t})

(defn- uncertain
  "An :info :add — the wire died mid-commit."
  [id t]
  {:type :info :f :add :value id :process 0 :time t})

(defn- read-ok
  "The final read returning `ids` — the {:ids #{...}} shape every
  surface's final read carries (the http lane's adds :register)."
  [ids t]
  {:type :ok :f :read :value {:ids (set ids)} :process 1 :time t})

(def ^:private crashed
  "A crash observation that lands: container down, the SIGKILL exit
  code, the lane's wire unreachable."
  {:running? false :exit-code 137 :wire-down? true})

(defn- kill
  "A fired kill whose evidence carries the kill instant (nemesis.clj
  records :killed-at right before docker kill; the op's own :time is
  post-verification). The evidence is built by the nemesis's own
  constructor, so the fixture can never drift from the production keys."
  [killed-at]
  {:type :info :f :start :process :nemesis
   :time (+ killed-at (long 5e8))
   :value (assoc (cnem/crash-evidence crashed) :killed-at killed-at)})

(defn- unfired-kill
  "A kill whose observation shows the container still up — :fired?
  false by the same constructor."
  [t]
  {:type :info :f :start :process :nemesis :time t
   :value (cnem/crash-evidence {:running? true :exit-code 0
                                :wire-down? false})})

(defn check
  "Runs `checker` over `history` with a dummy test."
  [chk history]
  (checker/check chk {:name "unit"} history {}))

;;; --- set-final-state (v1 semantics) -----------------------------------------

(deftest set-final-state-passes-when-acked-survives
  (let [h [(invoke-add 1 0) (ack 1 (* 3 sec))
           (invoke-add 2 (* 4 sec)) (ack 2 (* 5 sec))
           (invoke-add 3 (* 6 sec)) (rejected 3 (* 7 sec))
           (invoke-add 4 (* 8 sec)) (uncertain 4 (* 9 sec))
           (kill (* 10 sec))
           (read-ok [1 2 4] (* 12 sec))]]
    (is (:valid? (check (cc/set-final-state) h)))))

(deftest set-final-state-fails-on-lost-ack
  (let [h [(invoke-add 1 0) (ack 1 (* 3 sec))
           (kill (* 10 sec))
           (read-ok [] (* 12 sec))]
        r (check (cc/set-final-state) h)]
    (is (not (:valid? r)))
    (is (= 1 (get-in r [:lost-acked-writes :count])))))

(deftest set-final-state-fails-on-ghost
  (let [h [(invoke-add 1 0) (ack 1 (* 3 sec))
           (read-ok [1 99] (* 12 sec))]
        r (check (cc/set-final-state) h)]
    (is (not (:valid? r)))
    (is (= 1 (get-in r [:ghost-ids :count])))))

(deftest set-final-state-fails-on-rejected-write-present
  (let [h [(invoke-add 1 0) (rejected 1 (* 3 sec))
           (read-ok [1] (* 12 sec))]
        r (check (cc/set-final-state) h)]
    (is (not (:valid? r)))
    (is (= 1 (get-in r [:rejected-but-present :count])))))

(deftest set-final-state-fails-without-final-read
  (let [r (check (cc/set-final-state) [(invoke-add 1 0) (ack 1 sec)])]
    (is (not (:valid? r)))
    (is (re-find #"no final read" (:error r)))))

;;; --- durability-bound -------------------------------------------------------

(deftest durability-bound-passes-with-zero-loss
  (let [h [(invoke-add 1 0) (ack 1 (* 3 sec))
           (invoke-add 2 (* 4 sec)) (ack 2 (* 5 sec))
           (kill (* 10 sec))
           (read-ok [1 2] (* 12 sec))]
        r (check (cc/durability-bound 1) h)]
    (is (:valid? r))
    (is (= 0 (get-in r [:lost-acked-writes :count])))))

(deftest durability-bound-passes-on-in-window-loss
  ;; Acked 0.4s before the kill: inside everysecond's 1s window.
  (let [h [(invoke-add 1 0) (ack 1 (* 30 sec))
           (invoke-add 2 (* 30.4 sec)) (ack 2 (* 30.5 sec))
           (kill (* 31 sec))
           (read-ok [1] (* 40 sec))]
        r (check (cc/durability-bound 1) h)]
    (is (:valid? r))
    (is (= 1 (get-in r [:lost-acked-writes :count])))
    (is (= 0 (get-in r [:lost-outside-loss-window :count])))))

(deftest durability-bound-fails-on-out-of-window-loss
  ;; Acked 3s before the kill — outside the 1s window, must survive.
  (let [h [(invoke-add 1 0) (ack 1 (* 27 sec))
           (invoke-add 2 (* 28 sec)) (ack 2 (* 28.1 sec))
           (kill (* 31 sec))
           (read-ok [2] (* 40 sec))]
        r (check (cc/durability-bound 1) h)]
    (is (not (:valid? r)))
    (is (= 1 (get-in r [:lost-outside-loss-window :count])))
    (is (= [1] (get-in r [:lost-outside-loss-window :sample])))))

(deftest durability-bound-window-scales-with-the-mode
  ;; The same history under snapshot's 2s window: the 1.9s-old ack is
  ;; now in-window and its loss is legal.
  (let [h [(invoke-add 1 0) (ack 1 (* 29.1 sec))
           (kill (* 31 sec))
           (read-ok [] (* 40 sec))]]
    (is (not (:valid? (check (cc/durability-bound 1) h)))
        "1.9s-old ack lost is a violation at everysecond's 1s window")
    (is (:valid? (check (cc/durability-bound 2) h))
        "the same loss is inside snapshot's 2s window")))

(deftest durability-bound-kill-before-ack-permits-nothing
  ;; The only kill fired before the write was even attempted.
  (let [h [(invoke-add 1 0) (ack 1 (* 30 sec))
           (kill (* 5 sec))
           (read-ok [] (* 40 sec))]
        r (check (cc/durability-bound 1) h)]
    (is (not (:valid? r)))
    (is (= 1 (get-in r [:lost-outside-loss-window :count])))))

(deftest durability-bound-unfired-kill-permits-nothing
  ;; A kill that never fired is not fault evidence — only fired kills
  ;; bound the window.
  (let [h [(invoke-add 1 0) (ack 1 (* 30 sec))
           (unfired-kill (* 30.5 sec))
           (read-ok [] (* 40 sec))]
        r (check (cc/durability-bound 1) h)]
    (is (not (:valid? r)))
    (is (= 0 (:kills-fired r)))
    (is (= 1 (get-in r [:lost-outside-loss-window :count])))))

(deftest durability-bound-fails-on-ghost-in-every-mode
  (let [h [(invoke-add 1 0) (ack 1 (* 30 sec))
           (kill (* 31 sec))
           (read-ok [1 99] (* 40 sec))]
        r (check (cc/durability-bound 1) h)]
    (is (not (:valid? r)))
    (is (= 1 (get-in r [:ghost-ids :count])))))

(deftest durability-bound-fails-on-incomplete-kill-evidence
  ;; A fired kill without its :killed-at instant: the bound refuses to
  ;; guess the window edge.
  (let [h [(invoke-add 1 0) (ack 1 (* 30 sec))
           {:type :info :f :start :process :nemesis :time (* 31 sec)
            :value (cnem/crash-evidence crashed)}
           (read-ok [] (* 40 sec))]
        r (check (cc/durability-bound 1) h)]
    (is (not (:valid? r)))
    (is (re-find #":killed-at" (:error r)))))

(deftest durability-bound-fails-on-ack-without-time
  (let [h [(invoke-add 1 0) (dissoc (ack 1 (* 30 sec)) :time)
           (kill (* 31 sec))
           (read-ok [] (* 40 sec))]
        r (check (cc/durability-bound 1) h)]
    (is (not (:valid? r)))
    (is (re-find #"completion times" (:error r)))))

;;; --- memory-vfs-control -----------------------------------------------------

(deftest memory-vfs-control-valid-when-acked-writes-are-lost
  (let [h [(invoke-add 1 0) (ack 1 (* 3 sec))
           (invoke-add 2 (* 4 sec)) (ack 2 (* 5 sec))
           (kill (* 10 sec))
           (read-ok [2] (* 12 sec))]
        r (check (cc/memory-vfs-control) h)]
    (is (:valid? r))
    (is (false? (:full-mode-invariant-held? r)))
    (is (= 1 (get-in r [:lost-acked-writes :count])))))

(deftest memory-vfs-control-invalid-when-nothing-is-lost
  ;; The control that fails to fail: the fault cannot bite, and every
  ;; green lane above it is vacuous.
  (let [h [(invoke-add 1 0) (ack 1 (* 3 sec))
           (kill (* 10 sec))
           (read-ok [1] (* 12 sec))]
        r (check (cc/memory-vfs-control) h)]
    (is (false? (:valid? r))
        "strictly boolean — a nil verdict would read as 'unknown', not red")
    (is (true? (:full-mode-invariant-held? r)))
    (is (re-find #"did not bite" (:error r)))))

(deftest memory-vfs-control-still-rejects-ghosts
  (let [h [(invoke-add 1 0) (ack 1 (* 3 sec))
           (kill (* 10 sec))
           (read-ok [1 99] (* 12 sec))]
        r (check (cc/memory-vfs-control) h)]
    (is (not (:valid? r)))
    (is (= 1 (get-in r [:ghost-ids :count])))))

(deftest memory-vfs-control-fails-without-final-read
  (let [r (check (cc/memory-vfs-control) [(invoke-add 1 0) (ack 1 sec)])]
    (is (not (:valid? r)))
    (is (re-find #"no final read" (:error r)))))

;;; --- fault-evidence ---------------------------------------------------------

(defn- restart
  "A recovery that came back."
  [t]
  {:type :info :f :stop :process :nemesis :time t
   :value {:recovered? true :identity "Neo4j/5.26.0" :restart-ms 10}})

(deftest fault-evidence-passes-when-every-fault-fired-and-recovered
  (let [r (check (cc/fault-evidence) [(kill (* 10 sec)) (restart (* 11 sec))])]
    (is (true? (:valid? r)))
    (is (= 1 (:kills-fired r)))))

(deftest fault-evidence-fails-on-unfired-kill
  (let [r (check (cc/fault-evidence) [(unfired-kill (* 10 sec))
                                      (restart (* 11 sec))])]
    (is (false? (:valid? r)))
    (is (= 1 (count (:faults-without-effect r))))))

(deftest fault-evidence-dispatches-the-key-on-f
  ;; A recovery carrying a fault's key is broken evidence machinery:
  ;; :stop must prove :recovered?, whatever else its value holds.
  (let [r (check (cc/fault-evidence)
                 [(kill (* 10 sec))
                  {:type :info :f :stop :process :nemesis :time (* 11 sec)
                   :value {:fired? true}}])]
    (is (false? (:valid? r)))
    (is (= [{:fired? true}] (:ops-without-evidence-key r)))))

;;; --- the DB setup guard, per compose rig -------------------------------------

(defn- serve-argv
  "The DB container's command as docker inspect reports it — the argv
  docker-compose.yml's db service renders, with `database` as the store
  (docker-compose.lazyfs.yml restates exactly this list with
  /mnt/lazyfs/graph, and carries no durability flag)."
  [database]
  ["--host" "0.0.0.0" "--database" database "--bolt" "7687"
   "--auth" "jepsen:jepsen" "--bolt-identity" "Neo4j/5.26.0"])

(defn- guard
  "Runs the real setup guard against a container whose docker inspect
  reports `env` and `argv` (redefined — offline, deterministic). Returns
  :ok, or the guard's ex-data when it refuses."
  [opts env argv]
  (with-redefs [docker/env-of (constantly env)
                docker/cmd-of (constantly argv)]
    (try (#'cdb/verify-container-config! (assoc opts :db-container "db"))
         :ok
         (catch clojure.lang.ExceptionInfo e (ex-data e)))))

(defn- compose-env
  "The DB container env both compose files render: the base file's
  environment list (the lazyfs override inherits it unchanged)."
  [mode checkpoint-interval-secs]
  {"DO_NOT_TRACK"                        "1"
   "RELATE_DURABILITY_MODE"          mode
   "RELATE_CHECKPOINT_INTERVAL_SECS" checkpoint-interval-secs})

(deftest guard-lazyfs-row-pins-the-lazyfs-store
  (is (= {:durability-mode "full" :database "/mnt/lazyfs/graph"}
         (get-in @#'cdb/lane-db-config [:lazyfs :lanes "full"])))
  (is (nil? (get-in @#'cdb/lane-db-config [:lazyfs :lanes "memory-vfs"]))
      "a :memory: store on the power-volatile disk isolates nothing"))

(deftest guard-passes-both-poweroff-halves
  ;; run-poweroff.sh's two invocations: the env it exports to compose
  ;; and the --lane / --checkpoint-interval-secs it passes the harness
  ;; are the same values, and the lazyfs override's argv carries the
  ;; lazyfs store.
  (testing "the full half (RELATE_DURABILITY_MODE=full, interval 300)"
    (is (= :ok (guard {:store :lazyfs :lane "full" :checkpoint-interval-secs 300}
                      (compose-env "full" "300")
                      (serve-argv "/mnt/lazyfs/graph")))))
  (testing "the control (RELATE_DURABILITY_MODE=snapshot, interval 3600)"
    (is (= :ok (guard {:store :lazyfs :lane "snapshot" :checkpoint-interval-secs 3600}
                      (compose-env "snapshot" "3600")
                      (serve-argv "/mnt/lazyfs/graph"))))))

(deftest guard-stays-a-real-assertion-on-the-lazyfs-rig
  (let [opts {:store :lazyfs :lane "full" :checkpoint-interval-secs 300}]
    (testing "the stock volume store is not the lazyfs rig"
      (is (= "/data/graph"
             (get-in (guard opts (compose-env "full" "300")
                            (serve-argv "/data/graph"))
                     [:actual :database]))))
    (testing "a control container under the full half's flags"
      (is (= {:durability-mode "full" :database "/mnt/lazyfs/graph"
              :checkpoint-interval-secs "300"}
             (:expected (guard opts (compose-env "snapshot" "3600")
                               (serve-argv "/mnt/lazyfs/graph"))))))
    (testing "the pre-fix blank checkpoint env"
      (is (= "" (get-in (guard opts (compose-env "full" "")
                               (serve-argv "/mnt/lazyfs/graph"))
                        [:actual :checkpoint-interval-secs]))))
    (testing "a lane with no lazyfs row is refused before docker is asked"
      (is (= {:store :lazyfs :lane "memory-vfs"}
             (guard (assoc opts :lane "memory-vfs")
                    (compose-env "full" "300")
                    (serve-argv "/mnt/lazyfs/graph")))))))

(deftest guard-volume-rig-rejects-the-lazyfs-store
  ;; The regression the lazyfs row fixes: the power-off container judged
  ;; against the volume rig's full row throws on the store path.
  (is (= {:expected "/data/graph" :actual "/mnt/lazyfs/graph"}
         (let [r (guard {:store :volume :lane "full" :checkpoint-interval-secs 300}
                        (compose-env "full" "300")
                        (serve-argv "/mnt/lazyfs/graph"))]
           {:expected (get-in r [:expected :database])
            :actual   (get-in r [:actual :database])})))
  (is (= :ok (guard {:store :volume :lane "full" :checkpoint-interval-secs 300}
                    (compose-env "full" "300")
                    (serve-argv "/data/graph")))))

(defn -main
  "Runs the suite (clojure -M:test) — the checker contracts here plus
  the compare lane's engine contracts (compare-test) — and exits 0 only
  when every test passes: the lane matrix's gate for both."
  [& _]
  (require 'jepsen.relate.compare-test)
  (let [{:keys [fail error]} (run-tests 'jepsen.relate.checker-test
                                        'jepsen.relate.compare-test)]
    (System/exit (if (zero? (+ fail error)) 0 1))))
