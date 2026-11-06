#!/usr/bin/env bash
# The ordered lane matrix (LABS-653 crash-recovery matrix): the Memory
# VFS negative control FIRST — a fault that cannot fail a known-bad
# config cannot validate a good one, so the control's evidence (acked
# writes lost under the same kill fault) is on record before any
# full-mode green is counted — then the relaxed-mode bound lanes, then
# the full-mode green last.
#
# Each lane is one ./run.sh invocation with the lane's env; a red lane
# aborts the matrix (set -e), because a lane that cannot pass makes
# every later green uninterpretable. Logs land under runs/ and the
# jepsen stores under store/relate-set-kill9-<lane>/.
#
# The snapshot lane runs a 2s checkpoint interval: the loss bound the
# checker asserts is then "acks in the last 2s before a kill may be
# lost" — small enough that a violation is a real finding, large enough
# that the interval really elapses between the 10s-spaced kills.
set -euo pipefail
cd "$(dirname "$0")"

# Isolate this matrix's compose project, DB container, and data volume
# from every other concurrent harness run on the same docker daemon
# (the lane program runs its nodes in parallel; two runs sharing the v1
# names kill each other's DB mid-run).
export JEPSEN_SUFFIX="relaxed-lanes"

# The product image the lanes run against: built from THIS repo root so
# it carries the ADR-0470 durability-mode wiring (the harness probes
# for it at setup and fails loudly on a stale image). A private tag,
# not the make-docker :latest — parallel harness work on a shared
# docker daemon may hold its own :latest; run.sh's RELATE_IMAGE
# default remains the stock tag for one-off runs.
IMAGE="relate-bolt:jepsen-lanes"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "building $IMAGE from the repo root (docker build -t $IMAGE ../..)"
  docker build -t "$IMAGE" ../..
fi
export RELATE_IMAGE="$IMAGE"

# The lane order IS the evidence order: control first, full green last.
echo "=== lane 1/4: memory-vfs (negative control — acked writes MUST be lost)"
LANE=memory-vfs  CHECKPOINT_INTERVAL_SECS=300 ./run.sh
echo "=== lane 2/4: everysecond (loss bound: ~1s of acked commits)"
LANE=everysecond CHECKPOINT_INTERVAL_SECS=300 ./run.sh
echo "=== lane 3/4: snapshot (loss bound: acks since the last checkpoint, interval 2s)"
LANE=snapshot    CHECKPOINT_INTERVAL_SECS=2   ./run.sh
echo "=== lane 4/4: full (acked writes must all survive)"
LANE=full        CHECKPOINT_INTERVAL_SECS=300 ./run.sh
echo "=== all four lanes green, in evidence order"
