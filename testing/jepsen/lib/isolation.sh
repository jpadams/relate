# Lane isolation: the ONE derivation of a lane's compose names from
# JEPSEN_SUFFIX (docker-compose.yml's ISOLATION note). Sourced — never
# executed — by every lane wrapper (run.sh, run-http.sh,
# run-poweroff.sh) AFTER it has applied its own JEPSEN_SUFFIX default
# (run.sh: none; run-http.sh: http-lane; run-poweroff.sh: poweroff).
#
# The lane program runs its nodes concurrently on one docker daemon;
# two runs sharing the fixed names kill each other's DB mid-run (one
# run's hermetic wipe empties the other's store). With a suffix this
# derives a private compose project, DB container, control container,
# data volume, and neo4j reference container (compare lane only):
#
#   COMPOSE_PROJECT_NAME                 relate-jepsen-<suffix>
#   DB_CONTAINER                         relate-jepsen-db-<suffix>
#   RELATE_JEPSEN_CONTROL_CONTAINER  relate-jepsen-control-<suffix>
#   NEO4J_CONTAINER                      relate-jepsen-neo4j-<suffix>
#   DATA_VOLUME                          relate-jepsen-data-<suffix>
#
# Unset or empty = the single-run names: DB_CONTAINER and DATA_VOLUME
# take docker-compose.yml's own defaults (the wrappers pass them to the
# harness flags), and every other name is left to compose's default.
# The m2 dependency cache is shared across the suffixed stock lanes
# (external volume); the power-off rig pins its own m2, and a suffixed
# project builds its own control image.
DB_CONTAINER="relate-jepsen-db"
DATA_VOLUME="relate-jepsen-data"
if [ -n "${JEPSEN_SUFFIX:-}" ]; then
  COMPOSE_PROJECT_NAME="relate-jepsen-$JEPSEN_SUFFIX"
  DB_CONTAINER="relate-jepsen-db-$JEPSEN_SUFFIX"
  DATA_VOLUME="relate-jepsen-data-$JEPSEN_SUFFIX"
  RELATE_JEPSEN_CONTROL_CONTAINER="relate-jepsen-control-$JEPSEN_SUFFIX"
  NEO4J_CONTAINER="relate-jepsen-neo4j-$JEPSEN_SUFFIX"
  export COMPOSE_PROJECT_NAME RELATE_JEPSEN_CONTROL_CONTAINER NEO4J_CONTAINER
fi
export DB_CONTAINER DATA_VOLUME
