# Behavioral tests for check-doc-examples.sh
#
# Locks the telemetry opt-out of the doc-example harness spawns: every CLI
# invocation the script makes runs the binary with DO_NOT_TRACK=1
# (ADR-0079 §6), so a CI doc-examples run never emits telemetry events.
# The pin is behavioral — a stub RELATE_BIN fails unless DO_NOT_TRACK=1
# is in its environment and exits 0 otherwise, so the example-count guard
# still passes; if the script ever drops the opt-out, every stub invocation
# fails and the script exits 1.
#
# Local run:
#   bats .github/scripts/tests/check-doc-examples.bats
#
# Requirements:
#   bats-core >= 1.5.0  (brew install bats-core)
#   python3 (for the extractor, as in CI)

bats_require_minimum_version 1.5.0

SCRIPT="$(cd "$(dirname "$BATS_TEST_FILENAME")/../../.." && pwd)/scripts/check-doc-examples.sh"

# ---------------------------------------------------------------------------
# setup / teardown
# ---------------------------------------------------------------------------

setup() {
  TEST_TMP="$(mktemp -d)"

  # Strict stub for the relate binary: fails unless DO_NOT_TRACK=1 is
  # in its environment, exits 0 otherwise (so the example-count guard
  # passes). Every invocation appends to $STUB_INVOCATIONS — the marker
  # that proves the stub really ran.
  STUB_INVOCATIONS="${TEST_TMP}/invocations"
  export STUB_INVOCATIONS
  STUB_BIN="${TEST_TMP}/relate"
  cat >"${STUB_BIN}" <<'STUB'
#!/usr/bin/env bash
if [ "${DO_NOT_TRACK:-}" != "1" ]; then
  echo "stub-relate: DO_NOT_TRACK=1 is not in my environment" >&2
  exit 1
fi
printf 'x' >> "${STUB_INVOCATIONS:?STUB_INVOCATIONS is unset}"
exit 0
STUB
  chmod +x "${STUB_BIN}"
}

teardown() {
  rm -rf "${TEST_TMP}"
}

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

@test "every doc-example CLI spawn carries DO_NOT_TRACK=1" {
  # -u DO_NOT_TRACK: a shell exporting the convention (consoledonottrack)
  # must not make this pass vacuous — the script's own export is the only
  # source under test.
  run env -u DO_NOT_TRACK RELATE_BIN="${STUB_BIN}" bash "${SCRIPT}" 2>&1
  echo "$output"
  [ "$status" -eq 0 ]
  # The explicit override won (not a target/ autodetect).
  grep -qF "binary: ${STUB_BIN}" <<< "$output"
  grep -qF "All documented examples executed cleanly." <<< "$output"
  # The stub really ran, so the pass above is not vacuous: every
  # invocation saw DO_NOT_TRACK=1, or the stub would have failed the
  # script (exit 1) and this test with it.
  [ -s "${STUB_INVOCATIONS}" ]
}
