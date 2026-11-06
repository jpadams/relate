#!/usr/bin/env bash
# Optional CPU-level metrics: perf stat always, plus an eBPF off-CPU/futex
# trace when bpftrace is installed (it traces while the loadtest runs).
# Only meaningful for the in-memory run; disk runs are fsync-bound.
#
# Usage: sudo ./scripts/perf/04-cpu-ebpf.sh [memory|disk]
set -euo pipefail
cd "$(dirname "$0")/../.."
source scripts/perf/_common.sh

perf_invocation "$@"

# The overlap: perf stat runs the loadtest in the background (its output
# captured to a file), and bpftrace's 5-second window starts while that
# workload is live — its comm-matched probes only see the loadtest while it
# runs. The metrics are printed after both finish, perf stat first.
stat_out=$(mktemp)
perf stat -d -- "$LOADTEST_BIN" "${LOADTEST_ARGS[@]}" > "$stat_out" 2>&1 &
stat_pid=$!
# An interrupted run must not orphan the background perf stat or its
# loadtest child (async jobs ignore SIGINT in a non-interactive shell);
# after a normal wait both are gone and the kills are no-ops (`|| true`:
# under `set -e` a no-match kill would otherwise turn the exit code into 1).
trap 'pkill -P "$stat_pid" 2>/dev/null || true; kill "$stat_pid" 2>/dev/null || true; rm -f "$stat_out"' EXIT

if command -v bpftrace >/dev/null 2>&1; then
  bpf_out=$(bpftrace -e "
    tracepoint:sched:sched_switch /args->prev_comm==\"$LOADTEST_COMM\"/ { @offcpu[args->next_comm] = count(); }
    tracepoint:syscalls:sys_enter_futex /comm==\"$LOADTEST_COMM\"/ { @futex = count(); }
    interval:s:5 { exit(); }
  ") || bpf_out="(bpftrace failed — often needs sudo; see error above)"
else
  bpf_out="(bpftrace not installed; skipped off-CPU/futex trace)"
fi

stat_rc=0
wait "$stat_pid" || stat_rc=$?
out=$(<"$stat_out")

echo "== perf stat: IPC, context switches, cache misses, branch misses =="
if (( stat_rc != 0 )); then
  printf '%s\n' "$out" >&2
  echo "perf stat failed (exit $stat_rc); its full output is above" >&2
  exit "$stat_rc"
fi
# `|| true` covers only the genuine no-match case; failures exited above.
grep -E "task-clock|context-switches|cpu-migrations|page-faults|instructions|branches|branch-misses|cache-references|cache-misses|seconds user|instructions per cycle|IPC|throughput" <<< "$out" || true

echo
echo "== bpftrace: off-CPU time and futex wait on the loadtest process =="
printf '%s\n' "$bpf_out"
