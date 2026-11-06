#!/usr/bin/env bash
# processing-memory-loadtest in the ADR-0070 container limits (the shared
# ceremony is scripts/baseline-container.sh, the build recipe
# Dockerfile.baseline). The lane's own settings are the store path, which the
# ceremony bind-mounts off the /tmp tmpfs whose size counts against the cgroup
# memory limit, and that tmpfs size.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
path=()
if [[ -n "${PROCESSING_MEMORY_URI:-}" ]]; then
  path=(--path "--uri=${PROCESSING_MEMORY_URI}")
fi

exec bash "${ROOT}/scripts/baseline-container.sh" \
  --label processing-memory \
  --bins "relate processing-memory-loadtest" \
  --cache-id processing-memory \
  --entrypoint processing-memory-loadtest \
  --image "${PROCESSING_MEMORY_IMAGE:-relate-processing-memory:local}" \
  --container "${PROCESSING_MEMORY_CONTAINER:-relate-processing-memory}" \
  --out "${HOST_OUT:-target/processing-memory-loadtest.json}" \
  --tmpfs "${PROCESSING_MEMORY_TMPFS:-1g}" \
  ${path[@]+"${path[@]}"} \
  -- "$@"
