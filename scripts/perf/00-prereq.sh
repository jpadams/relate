#!/usr/bin/env bash
# Verify the host has everything the perf scripts need. Exits 1 with the
# missing pieces printed, 0 when ready. Run this first on a new Linux host.
set -euo pipefail

missing=0
need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "MISSING: $1   ($2)"
    missing=1
  fi
}

need cargo "rustup / your toolchain"
need perf "linux-tools / linux-perf package (e.g. apt install linux-tools-generic)"

# perf_event_paranoid: unprivileged perf sampling needs <= 1 (-1 is best).
if [[ -f /proc/sys/kernel/perf_event_paranoid ]]; then
  val=$(cat /proc/sys/kernel/perf_event_paranoid)
  if (( val > 1 )); then
    echo "WARN: /proc/sys/kernel/perf_event_paranoid=$val (>1)."
    echo "      Unprivileged sampling is limited. Either:"
    echo "        sudo sysctl kernel.perf_event_paranoid=-1"
    echo "      or run the perf scripts under sudo."
  fi
fi

# eBPF CPU metrics are optional; only flag if the user asked for them.
if [[ "${1:-}" == "--ebpf" ]]; then
  if ! command -v bpftrace >/dev/null 2>&1 && ! command -v bpftool >/dev/null 2>&1; then
    echo "MISSING (ebpf): bpftrace or bpftool   (apt install bpftrace / linux-tools-common)"
    missing=1
  fi
fi

# Debug symbols make perf output readable. Non-fatal here; the build script handles it.
echo "rustc: $(rustc --version 2>/dev/null || echo 'not found')"
echo "perf:  $(perf --version 2>/dev/null || echo 'not found')"
[[ "$missing" -eq 0 ]] && echo "OK: host ready" || exit 1
