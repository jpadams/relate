#!/usr/bin/env bash
# One-shot run of ONE Jepsen lane: assumes the product image (default
# relate-bolt:latest — `make docker` builds it; override with
# RELATE_IMAGE) already exists, starts the DB node configured for
# the lane, runs the harness in the control container, and tees the run
# log under runs/.
#
# Workloads (WORKLOAD, default set):
#
#   set      the crash-durability set family — 5 clients CREATE unique
#            ids under the NEMESIS fault schedule (default kill9: kill
#            -9 / restart), checked by the LANE's final-state contract.
#   counter  the Elle list-append lane — isolation under concurrency,
#            NO faults by default (NEMESIS=kill adds the crash cycle).
#   bank     the Elle rw-register lane, additionally checked by jepsen's
#            bank checker (conservation, non-negativity).
#   compare  the Neo4j differential workload (on demand only, never
#            per-PR): a random graph + ops over the shared openCypher
#            floor against BOTH Relate and the neo4j:5.26.0
#            reference container; exit 1 on any divergence, shrunk to a
#            minimal case and recorded in store/<test>/divergences.json.
#
# Fault schedules (NEMESIS, the set family): kill9 (default), pause,
# sigterm-kill9, mid-checkpoint — the schedule table itself lives in
# src/jepsen/relate/schedule.clj; this launcher maps the one fact
# compose cannot read from Clojure: which env the DB container needs
# for the schedule (mid-checkpoint's 5s checkpoint interval). The Elle
# lanes take kill|none instead. The lazyfs power-off lane has its own
# wrapper (run-poweroff.sh — it needs the lazyfs compose rig); the HTTP
# surface lane's is run-http.sh.
#
# Durability lanes (LANE, the set family) — the final-state contract
# checked, and the compose env the DB container must carry (db.clj
# asserts the container really runs them at setup):
#
#   LANE           RELATE_DURABILITY_MODE  RELATE_DATABASE
#   full           full                        /data/graph
#   everysecond    everysecond                 /data/graph
#   snapshot       snapshot                    /data/graph
#   memory-vfs     full                        :memory:  (negative control)
#
# CHECKPOINT_INTERVAL_SECS (default 300) is the timed checkpoint period
# — also the snapshot lane's loss window (pass it small, e.g. 2, to
# shrink the bound; lanes.sh does). A schedule that states its own
# interval (mid-checkpoint's 5s) wins over it.
#
# Elle/compare knobs: COUNTERS, ACCOUNTS, MAX_TRANSFER; OPS, SEED (a
# fresh timestamp by default, recorded in the test name — pass SEED to
# replay a recorded run exactly). Shared knobs: TIME_LIMIT,
# CONCURRENCY, NEMESIS_INTERVAL. Extra argv passes through to the
# harness CLI.
#
# JEPSEN_SUFFIX derives a private compose project, DB container,
# control container, data volume, and — for the compare lane — neo4j
# reference container (relate-jepsen-<suffix>…) for concurrent runs
# on one shared docker daemon — two runs sharing the fixed names kill
# each other's DB mid-run (one run's hermetic wipe empties the other's
# store). Unset = the single-run names. The control image and the m2
# dependency cache stay shared.
#
# Exit status is the harness's: 0 = every check passed (for memory-vfs
# that means the control LOST acknowledged writes, as it must).
set -euo pipefail
cd "$(dirname "$0")"

WORKLOAD="${WORKLOAD:-set}"
NEMESIS="${NEMESIS:-}"
LANE="${LANE:-full}"
CONCURRENCY="${CONCURRENCY:-5}"
NEMESIS_INTERVAL="${NEMESIS_INTERVAL:-5}"
CHECKPOINT_INTERVAL_SECS="${CHECKPOINT_INTERVAL_SECS:-300}"
RELATE_IMAGE="${RELATE_IMAGE:-relate-bolt:latest}"
export RELATE_IMAGE

case "$WORKLOAD" in
  set|counter|bank|compare) ;;
  *) echo "unknown WORKLOAD: $WORKLOAD (set|counter|bank|compare)" >&2; exit 2 ;;
esac

# The schedule's own compose needs: mid-checkpoint's kills must land on
# a store checkpointing every 5s — the harness's setup guard asserts
# the container really carries it. This is the one schedule fact that
# overrides CHECKPOINT_INTERVAL_SECS.
case "$NEMESIS" in
  "") ;;
  mid-checkpoint) export RELATE_CHECKPOINT_INTERVAL_SECS=5 ;;
  kill9|pause|sigterm-kill9|kill|none) ;;
  *)
    echo "unknown NEMESIS: $NEMESIS (set family: kill9|pause|sigterm-kill9|mid-checkpoint; Elle lanes: kill|none)" >&2
    exit 2 ;;
esac
export RELATE_CHECKPOINT_INTERVAL_SECS="${RELATE_CHECKPOINT_INTERVAL_SECS:-$CHECKPOINT_INTERVAL_SECS}"

# Per-lane run lengths: the set family's longer schedules need longer
# runs (SIGTERM waits out its clean shutdown, mid-checkpoint needs
# several checkpoint periods); the Elle lanes' certified invocation is
# 5+ clients for 5+ minutes; compare is op-count-bounded.
if [ -z "${TIME_LIMIT:-}" ]; then
  case "$WORKLOAD" in
    set)
      case "$NEMESIS" in
        sigterm-kill9|mid-checkpoint) TIME_LIMIT=110 ;;
        *)                            TIME_LIMIT=70  ;;
      esac ;;
    counter|bank) TIME_LIMIT=330 ;;
    compare)      TIME_LIMIT=600 ;;
  esac
fi

# The durability lane's compose env (the set family's lanes; harmless —
# and asserted — for the others, which run the default full config).
case "$LANE" in
  full|everysecond|snapshot)
    export RELATE_DURABILITY_MODE="$LANE"
    export RELATE_DATABASE="/data/graph"
    ;;
  memory-vfs)
    # The negative control isolates the VFS: full durability mode on a
    # store with no durable medium — the known-bad config the kill
    # fault must be able to fail.
    export RELATE_DURABILITY_MODE="full"
    export RELATE_DATABASE=":memory:"
    ;;
  *)
    echo "unknown LANE: $LANE (full|everysecond|snapshot|memory-vfs)" >&2
    exit 2 ;;
esac

# Lane isolation: a private project, DB container, control container,
# data volume, and neo4j reference container (compare only) from
# JEPSEN_SUFFIX when one is given (unset = the single-run names) — the
# one derivation every lane wrapper sources.
. ./lib/isolation.sh

# Lane flags, assembled per workload (the CLI validates the ranges).
LANE_FLAGS=(--workload "$WORKLOAD"
            --lane "$LANE"
            --checkpoint-interval-secs "$CHECKPOINT_INTERVAL_SECS"
            --db-container "$DB_CONTAINER"
            --data-volume "$DATA_VOLUME"
            --concurrency "$CONCURRENCY"
            --time-limit "$TIME_LIMIT"
            --nemesis-interval "$NEMESIS_INTERVAL")
# The fault selection passes through only when the operator chose one —
# each workload family then defaults it its own way (set: the kill9
# schedule; Elle: no faults).
[ -z "$NEMESIS" ] || LANE_FLAGS+=(--nemesis "$NEMESIS")
case "$WORKLOAD" in
  counter) LANE_FLAGS+=(--counters "${COUNTERS:-4}") ;;
  bank)    LANE_FLAGS+=(--accounts "${ACCOUNTS:-16}"
                        --max-transfer "${MAX_TRANSFER:-5}") ;;
  compare) LANE_FLAGS+=(--ops "${OPS:-300}" --seed "${SEED:-$(date +%s)}") ;;
esac

# The compose invocation, classified once: --profile compare adds the
# neo4j reference service, so the compare lane brings it up AND takes it
# down through the same profile.
COMPOSE=(docker compose)
UP_SERVICES=(db)
if [ "$WORKLOAD" = compare ]; then
  COMPOSE+=(--profile compare)
  # Both DBs must be up before the control container starts comparing.
  UP_SERVICES+=(neo4j)
fi

RUN_LOG="runs/$(date -u +%Y%m%dT%H%M%SZ)-$WORKLOAD.log"

# The control image is built once and cached; the harness itself is
# bind-mounted, so code edits need no rebuild.
docker compose build control

# A crashed earlier run can leave the DB container paused; compose
# cannot start a paused container, so thaw it first (idempotent
# pre-step, like the harness's own stop!).
docker unpause "$DB_CONTAINER" 2>/dev/null || true

"${COMPOSE[@]}" up -d "${UP_SERVICES[@]}"

mkdir -p runs
set +e
docker compose run --rm control \
    clojure -M:run test \
    "${LANE_FLAGS[@]}" \
    "$@" \
    2>&1 | tee "$RUN_LOG"
STATUS="${PIPESTATUS[0]}"
set -e

# Keep the store directory (jepsen history + fressian test file) — it
# is bind-mounted into the repo at testing/jepsen/store/ — and take the
# project down with the volumes intact so the next run starts hermetic.
"${COMPOSE[@]}" down

echo "run log: $RUN_LOG"
echo "jepsen store: $(ls -dt store/*/ | head -1)"
[ -z "${JEPSEN_SUFFIX:-}" ] || echo "isolated run: project $COMPOSE_PROJECT_NAME, volume $DATA_VOLUME (remove with: docker volume rm $DATA_VOLUME)"
exit "$STATUS"
