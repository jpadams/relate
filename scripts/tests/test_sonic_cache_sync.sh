#!/usr/bin/env bash
# sonic-cache-sync rejects a bad subdir before it touches docker, and the
# seed/save pair carries the cache for both modes: the bind mode (the volume
# IS the host tree — the seed moves the restored cache under the stage subdir
# and the build writes through) and the fallback copy mode (the volume holds
# the cache between the two invocations).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
sync="$root/scripts/sonic-cache-sync.sh"

if "$sync" seed '../outside' >/dev/null 2>&1; then
  echo "expected a bad subdir to fail" >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
  echo "docker unavailable; skipped the volume round trip"
  exit 0
fi

run() { # run <mode> <subdir> [bind]
  local mode="$1" sub="$2" bind="${3:-auto}"
  GITHUB_WORKSPACE="$work" \
  CARGO_HOME="$work/.cargo" \
  SONIC_CACHE_VOLUME_PREFIX="$prefix" \
  SONIC_CACHE_SYNC_BIND="$bind" \
    "$sync" "$mode" "$sub"
}

check() { # check <subdir> <bind> — the round trip, in the given mode
  local sub="$1" bind="$2" label="copy"
  [[ "$bind" == "1" ]] && label="bind"

  work="$(mktemp -d)"
  mkdir -p "$work/target/debug" "$work/.cargo/registry/cache"
  echo 'cached-artifact' > "$work/target/debug/lib.rlib"
  echo 'crate' > "$work/.cargo/registry/cache/crate.tgz"

  run seed "$sub" "$bind"

  if [[ "$bind" == "1" ]]; then
    # Bind mode: the restored flat entries moved under the stage subdir.
    [[ -f "$work/target/$sub/debug/lib.rlib" ]] || {
      echo "[$label] seed did not move the restored cache under $sub/" >&2; exit 1; }
    # The stage writes through: simulate a build artifact.
    mkdir -p "$work/target/$sub/debug/deps"
    echo built > "$work/target/$sub/debug/deps/x.rlib"
  else
    # Copy mode: the host tree is staged into the volume; drop it so the save
    # has to bring it back.
    [[ -f "$work/target/debug/lib.rlib" ]] || {
      echo "[$label] seed did not stage the host tree" >&2; exit 1; }
    rm -rf "$work/target" "$work/.cargo"
  fi

  run save "$sub" "$bind"

  local got
  got="$(cat "$work/target/$sub/debug/lib.rlib" 2>/dev/null || cat "$work/target/debug/lib.rlib" 2>/dev/null || true)"
  if [[ "$got" != "cached-artifact" ]]; then
    echo "[$label] the artifact did not survive the round trip (got '$got')" >&2
    exit 1
  fi
  if [[ "$bind" == "1" && "$(cat "$work/target/$sub/debug/deps/x.rlib")" != "built" ]]; then
    echo "[$label] the write-through artifact is missing" >&2; exit 1
  fi
  echo "[$label] round trip ok"
  rm -rf "$work"
}

prefix="gosonic-cache-test-$$"
trap 'rm -rf "${work:-}"; docker volume rm -f "${prefix}-cargo-target" "${prefix}-cargo-home" "${prefix}-bind-probe-$$" >/dev/null 2>&1 || true' EXIT

# Bind mode (forced on: the probe itself is exercised by the default run
# below), and the fallback copy mode.
check test-engine 1
check test-engine 0

# A poisoned restore: the archive carries both <sub>/<name> and a flat <name>
# (a stage wrote the workspace target/ flat during the previous run — the
# baseline-env docker build's target/release). The seed must replace the
# seeded copy, not die ENOTEMPTY.
work="$(mktemp -d)"
mkdir -p "$work/target/test-storage/nextest" "$work/target/nextest" "$work/.cargo/registry"
echo seeded > "$work/target/test-storage/nextest/store.json"
echo flat > "$work/target/nextest/store.json"
run seed test-storage 1
[[ "$(cat "$work/target/test-storage/nextest/store.json")" == "flat" ]] || {
  echo "[poisoned] the flat entry did not replace the seeded one" >&2; exit 1; }
[[ ! -e "$work/target/nextest" ]] || {
  echo "[poisoned] the flat entry stayed behind" >&2; exit 1; }
rm -rf "$work"
echo "[poisoned] mixed restore ok"
echo "sonic-cache-sync ok"
