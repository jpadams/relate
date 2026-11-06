#!/usr/bin/env bash
# memory-shape-loadtest in the ADR-0070 container limits (the shared ceremony
# is scripts/baseline-container.sh, the build recipe Dockerfile.baseline). The
# lane's own setting is the store path, which the ceremony bind-mounts off the
# /tmp tmpfs whose size counts against the cgroup memory limit.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
path=()
if [[ -n "${MEMORY_SHAPE_URI:-}" ]]; then
  path=(--path "--uri=${MEMORY_SHAPE_URI}")
fi

exec bash "${ROOT}/scripts/baseline-container.sh" \
  --label memory-shape \
  --bins "memory-shape-loadtest relate" \
  --cache-id memory-shape \
  --entrypoint memory-shape-loadtest \
  --image "${MEMORY_SHAPE_IMAGE:-relate-memory-shape:local}" \
  --container "${MEMORY_SHAPE_CONTAINER:-relate-memory-shape}" \
  --out "${HOST_OUT:-target/memory-shape-loadtest.json}" \
  ${path[@]+"${path[@]}"} \
  -- "$@"
