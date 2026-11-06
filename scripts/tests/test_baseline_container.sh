#!/bin/sh
# The shared baseline container ceremony's own rules (ADR-0495), asserted
# without a Docker daemon: scripts/baseline-container.sh guards its flags
# before it looks for docker, so the refusals below must be the authored
# errors and must not reach the docker stub at all.
#
# Output and exit contract: scripts/tests/lib/tap.sh.

set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
CEREMONY="$ROOT/scripts/baseline-container.sh"

. "$ROOT/scripts/tests/lib/tap.sh"

tap_sandbox relate-baseline-container-test

stubbin="$sandbox/stub-bin"
docker_log="$sandbox/docker.log"
mkdir -p "$stubbin"
cat >"$stubbin/docker" <<'STUB'
#!/bin/sh
printf '%s\n' "$*" >>"$DOCKER_LOG"
# The two base images are already present and the build succeeds, so the
# ceremony reaches the container create, where this stub stops it.
case "$1" in
  image | build) exit 0 ;;
esac
exit 1
STUB
chmod +x "$stubbin/docker"

# One ceremony invocation with further ceremony flags. The report path and
# the build args are the minimum every lane passes; nothing here runs a
# binary.
run_ceremony() { # the rest = further ceremony flags
  PATH="$stubbin:$PATH" DOCKER_LOG="$docker_log" \
    bash "$CEREMONY" \
    --label flag-guard \
    --bins "memory-shape-loadtest relate" \
    --cache-id flag-guard \
    --entrypoint memory-shape-loadtest \
    --image flag-guard:test \
    --container flag-guard \
    --out "$sandbox/report.json" \
    "$@" -- >/dev/null 2>"$sandbox/stderr"
}

# One refused ceremony invocation: a non-zero exit, the authored text, and
# no docker call at all (the stub would have logged one).
expect_refusal() { # $1 = case description, $2 = the refusal text, the rest = flags
  desc="$1"
  want="$2"
  shift 2
  : >"$docker_log"
  if run_ceremony "$@"; then
    tap_not_ok "$desc"
  elif grep -q -- "$want" "$sandbox/stderr" && [ ! -s "$docker_log" ]; then
    tap_ok "$desc"
  else
    tap_not_ok "$desc"
  fi
}

# An unknown ceremony flag, a malformed --path and an empty required flag
# are each the authored refusal, and no docker call happens first.
expect_refusal "an unknown ceremony flag is the authored refusal, no docker" \
  "unknown argument: --no-such-flag" --no-such-flag
expect_refusal "a --path without = is the authored refusal, no docker" \
  "--path takes <flag>=<value>" --path "--uri"
expect_refusal "a --path flag with no value is the authored refusal, no docker" \
  "--path --uri has no value" --path "--uri="
expect_refusal "an empty required flag is the usage refusal, no docker" \
  "usage: baseline-container.sh" --entrypoint ""

# The memory-shape lane script's own contract: its store path knob arrives as
# env (MEMORY_SHAPE_URI — the lane declares it as the ceremony's --path
# mount), and every other argument reaches the bench untouched.
shape_lane="$ROOT/scripts/memory-shape-docker.sh"
lane_store="$sandbox/lane/store"
mkdir -p "$lane_store"

run_shape_lane() { # the rest = the lane's arguments
  : >"$docker_log"
  PATH="$stubbin:$PATH" DOCKER_LOG="$docker_log" \
    MEMORY_SHAPE_URI="$lane_store" \
    bash "$shape_lane" "$@" >/dev/null 2>"$sandbox/stderr" || true
}

run_shape_lane --logical-mb 32
lane_create="$(grep '^create ' "$docker_log" || true)"
if [ -n "${lane_create}" ] \
    && grep -q -F -- "-v ${lane_store}:${lane_store}" "$docker_log" \
    && grep -q -F -- "--uri ${lane_store}" "$docker_log"; then
  tap_ok "MEMORY_SHAPE_URI is the lane's store bind mount"
else
  tap_not_ok "MEMORY_SHAPE_URI is the lane's store bind mount"
fi
if [ -n "${lane_create}" ] && grep -q -F -- "--logical-mb 32" "$docker_log"; then
  tap_ok "the bench's own arguments are forwarded"
else
  tap_not_ok "the bench's own arguments are forwarded"
fi

tap_done
