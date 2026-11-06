#!/usr/bin/env bash
# Render a flamegraph SVG from /tmp/lt-<mode>.perf.
# Prefers inferno (cargo install inferno); falls back to brendangregg/FlameGraph
# (git clone https://github.com/brendangregg/FlameGraph, set FLAMEGRAPH_DIR).
set -euo pipefail
mode="${1:-memory}"
data="/tmp/lt-${mode}.perf"
out="/tmp/lt-${mode}.svg"
[[ -f "$data" ]] || { echo "missing $data; run 02-run-loadtest.sh $mode first"; exit 1; }

if command -v inferno-flamegraph >/dev/null 2>&1; then
  perf script -i "$data" 2>/dev/null | inferno-collapse-perf | inferno-flamegraph > "$out"
elif command -v inferno-collapse-perf >/dev/null 2>&1 && [[ -n "${FLAMEGRAPH_DIR:-}" ]]; then
  perf script -i "$data" 2>/dev/null | inferno-collapse-perf \
    | "$FLAMEGRAPH_DIR/flamegraph.pl" > "$out"
elif [[ -n "${FLAMEGRAPH_DIR:-}" ]]; then
  perf script -i "$data" 2>/dev/null \
    | "$FLAMEGRAPH_DIR/stackcollapse-perf.pl" \
    | "$FLAMEGRAPH_DIR/flamegraph.pl" > "$out"
else
  echo "install inferno (cargo install inferno) or set FLAMEGRAPH_DIR to a FlameGraph checkout"
  exit 1
fi
echo "wrote $out"
