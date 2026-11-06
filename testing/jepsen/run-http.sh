#!/usr/bin/env bash
# One-shot green run of the HTTP Query API v2 lane (LABS-655): assumes
# `make docker` already built the product image, then starts the DB
# node (the entrypoint itself appends --http 7474), runs the harness in
# the control container against the HTTP wire (5 clients, 70s, kill -9
# / restart cycle every ~5s up/down), and tees the run log under runs/.
#
# Exit status is the harness's: 0 = every check passed.
set -euo pipefail
cd "$(dirname "$0")"

TIME_LIMIT="${TIME_LIMIT:-70}"
CONCURRENCY="${CONCURRENCY:-5}"
NEMESIS_INTERVAL="${NEMESIS_INTERVAL:-5}"
RUN_LOG="runs/http-$(date -u +%Y%m%dT%H%M%SZ).log"

# Lane isolation (docker-compose.yml): this lane derives its project,
# DB container, control container, and data volume from JEPSEN_SUFFIX
# (default http-lane) through the one derivation every lane wrapper
# sources (lib/isolation.sh), so a concurrently running sibling lane's
# compose up / hermetic wipe can never resurrect or empty this lane's
# mid-run DB (and vice versa). The control image and the m2 dependency
# cache stay shared. The harness flags must name the same container
# and volume compose creates.
JEPSEN_SUFFIX="${JEPSEN_SUFFIX:-http-lane}"
. ./lib/isolation.sh

# The control image is built once and cached; the harness itself is
# bind-mounted, so code edits need no rebuild.
docker compose build control
docker compose up -d db

mkdir -p runs
set -o pipefail
docker compose run --rm control \
    clojure -M:run test \
    --surface http \
    --db-container "$DB_CONTAINER" \
    --data-volume "$DATA_VOLUME" \
    --concurrency "$CONCURRENCY" \
    --time-limit "$TIME_LIMIT" \
    --nemesis-interval "$NEMESIS_INTERVAL" \
    2>&1 | tee "$RUN_LOG"

# Keep the store directory (jepsen history + fressian test file) — it is
# bind-mounted into the repo at testing/jepsen/store/ — and leave the
# compose project down but the volumes intact for inspection.
docker compose down
echo "run log: $RUN_LOG"
echo "jepsen store: $(ls -dt store/relate-set-kill9-http-*/* | head -1)"
