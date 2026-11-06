#!/usr/bin/env bash
# Turn a perf record into a crate-layer summary so you can see which engine
# layer owns CPU. Reads /tmp/lt-<mode>.perf (from 02-run-loadtest.sh).
#
# Usage: ./scripts/perf/03-bottleneck-summary.sh [memory|disk]
set -euo pipefail
mode="${1:-memory}"
data="/tmp/lt-${mode}.perf"
[[ -f "$data" ]] || { echo "missing $data; run 02-run-loadtest.sh $mode first"; exit 1; }

echo "== top functions (self) =="
perf report -i "$data" --stdio --no-children 2>/dev/null \
  | grep -E '^\s*[0-9]+\.[0-9]+%' | head -25

echo
echo "== share by engine layer (inclusive, from call graph) =="
perf report -i "$data" --stdio --children 2>/dev/null \
  | grep -E '^\s*[0-9]+\.[0-9]+%' \
  | grep -oE 'relate_[a-z_]+' | sort | uniq -c | sort -rn | head -20 \
  | while read -r n sym; do printf "%6d  %s\n" "$n" "$sym"; done || true

echo
echo "== lock/wait share =="
perf report -i "$data" --stdio --no-children 2>/dev/null \
  | grep -iE 'psynch|futex|wait|lock|fsync|fcntl|sync_all' | head -12

echo
echo "Hint: if 'commit::CommitWaiter' or futex dominates in memory mode, that is"
echo "the single-writer serialize point (ADR-0014), not a removable layer."
