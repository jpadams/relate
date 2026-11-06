#!/usr/bin/env bash
# Build the loadtest binaries with debug symbols and full optimization.
# perf needs frame info to attribute samples; without this you get hex addresses.
set -euo pipefail
cd "$(dirname "$0")/../.."

echo "Building fraud-loadtest / identity-loadtest with symbols..."
# The canonical symbolized-release profile (ADR-0244): release code with
# line tables and strip=none — the same mechanism make query-latency-profile
# uses. Binaries land in target/profiling/.
cargo build --profile profiling -p relate-datasets \
  --bin fraud-loadtest --bin identity-loadtest

echo "Done. Binaries:"
ls -la target/profiling/fraud-loadtest target/profiling/identity-loadtest
