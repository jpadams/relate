# Behavioral tests for install-opencode.sh
#
# Locks the fail-closed install (ADR-0258 tarball-plus-sha256 pattern):
# a tarball whose content does not match the pinned OPENCODE_SHA256 aborts
# the install before anything is extracted, and the pins baked into the
# script keep their expected shapes (semver version, 64-hex sha256).
#
# Local run:
#   bats .github/scripts/tests/install-opencode.bats
#
# Requirements:
#   bats-core >= 1.5.0  (brew install bats-core)

bats_require_minimum_version 1.5.0

SCRIPT="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)/install-opencode.sh"

# ---------------------------------------------------------------------------
# setup / teardown
# ---------------------------------------------------------------------------

setup() {
  TEST_TMP="$(mktemp -d)"

  STUBS_DIR="${TEST_TMP}/stubs"
  mkdir -p "${STUBS_DIR}"

  # curl stub — writes a garbage tarball to the -o path and exits 0.
  # No network calls.
  cat >"${STUBS_DIR}/curl" <<'STUB'
#!/usr/bin/env bash
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    *) shift ;;
  esac
done
printf 'garbage-not-the-pinned-tarball' > "${out:?no -o argument}"
exit 0
STUB
  chmod +x "${STUBS_DIR}/curl"

  INSTALL_DIR="${TEST_TMP}/install"
  export STUBS_DIR TEST_TMP INSTALL_DIR
}

teardown() {
  rm -rf "${TEST_TMP}"
  rm -f /tmp/opencode.tgz
}

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

@test "fails closed when the tarball fails sha256 verification" {
  run env PATH="${STUBS_DIR}:${PATH}" OPENCODE_INSTALL_DIR="${INSTALL_DIR}" \
    bash "${SCRIPT}" 2>&1
  echo "$output"
  [ "$status" -ne 0 ]
  grep -qF "FAILED" <<< "$output"
  # Nothing was extracted.
  [ ! -e "${INSTALL_DIR}/opencode" ]
}

@test "pinned OPENCODE_VERSION matches a semver shape" {
  grep -qE '^OPENCODE_VERSION=[0-9]+\.[0-9]+\.[0-9]+$' "${SCRIPT}"
}

@test "pinned OPENCODE_SHA256 is exactly 64 lowercase hex chars" {
  grep -qE '^OPENCODE_SHA256=[0-9a-f]{64}$' "${SCRIPT}"
}
