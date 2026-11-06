# Behavioral tests for release-notes-prepend.sh
#
# Locks the merge policy for the idempotent GitHub release update
# (ADR-0124, amended by ADR-0125): prepend a fresh `## Release notes`
# block fully delimited by `<!-- release-notes:start -->` /
# `<!-- release-notes:end -->` HTML comment markers (start marker is
# the block's first line, end marker its last, heading inside),
# replacing a previous generated block only when a start marker is
# followed by an end marker. The strip is a single inclusive range
# delete from the start-marker line through the end-marker line, so
# two consecutive runs are a byte fixpoint by construction; notes
# files containing either marker string are refused; CR characters
# are stripped from the current body before marker matching so a
# CRLF body cannot degrade replace into stacking.
#
# Local run:
#   bats .github/scripts/tests/release-notes-prepend.bats
#
# Requirements:
#   bats-core >= 1.5.0  (brew install bats-core)

bats_require_minimum_version 1.5.0

SCRIPT="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)/release-notes-prepend.sh"
FIXTURES_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")" && pwd)/fixtures"

NOTES="${FIXTURES_DIR}/release-notes-sample-notes.md"
START_MARKER='<!-- release-notes:start -->'
END_MARKER='<!-- release-notes:end -->'

# ---------------------------------------------------------------------------
# setup / teardown
# ---------------------------------------------------------------------------

setup() {
  TEST_TMP="$(mktemp -d)"
  export TEST_TMP
}

teardown() {
  rm -rf "${TEST_TMP}"
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Run the prepend script. Args: current_body_file notes_file out_file
run_prepend() {
  bash "${SCRIPT}" "$@" 2>&1
}

# Build the expected merged body: fully delimited generated block
# followed directly by the kept body (no trailing blank separator
# after the end marker).
# Args: notes_file kept_body_file expected_file
write_expected() {
  {
    printf '%s\n## Release notes\n\n' "${START_MARKER}"
    cat "$1"
    printf '\n%s\n' "${END_MARKER}"
    cat "$2"
  } > "$3"
}

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

@test "empty current body is a pure prepend" {
  : > "${TEST_TMP}/current.md"
  run run_prepend "${TEST_TMP}/current.md" "${NOTES}" "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 0 ]

  write_expected "${NOTES}" /dev/null "${TEST_TMP}/expected.md"
  diff -u "${TEST_TMP}/expected.md" "${TEST_TMP}/out.md"
}

@test "body with a previous generated section is replaced, not stacked" {
  run run_prepend "${FIXTURES_DIR}/release-notes-body-with-section.md" \
    "${NOTES}" "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 0 ]

  # Exactly one generated heading and one of each marker.
  [ "$(grep -cx '## Release notes' "${TEST_TMP}/out.md")" -eq 1 ]
  [ "$(grep -cxF "${START_MARKER}" "${TEST_TMP}/out.md")" -eq 1 ]
  [ "$(grep -cxF "${END_MARKER}" "${TEST_TMP}/out.md")" -eq 1 ]

  # The stale section is gone; the hand-written body survives.
  ! grep -qF "Stale feature" "${TEST_TMP}/out.md"
  grep -qF "Hand-written body text" "${TEST_TMP}/out.md"

  # Expected body: generated block + everything after the previous
  # end marker (the fixture's generated block spans lines 1-7; the
  # hand-written body follows the end-marker line directly).
  awk 'NR > 7' "${FIXTURES_DIR}/release-notes-body-with-section.md" \
    > "${TEST_TMP}/kept.md"
  write_expected "${NOTES}" "${TEST_TMP}/kept.md" "${TEST_TMP}/expected.md"
  diff -u "${TEST_TMP}/expected.md" "${TEST_TMP}/out.md"
}

@test "double application is a byte fixpoint" {
  run run_prepend "${FIXTURES_DIR}/release-notes-body-with-section.md" \
    "${NOTES}" "${TEST_TMP}/first.md"
  echo "$output"
  [ "$status" -eq 0 ]

  run run_prepend "${TEST_TMP}/first.md" "${NOTES}" "${TEST_TMP}/second.md"
  echo "$output"
  [ "$status" -eq 0 ]

  # Load-bearing assertion: two consecutive runs are byte-identical.
  diff -u "${TEST_TMP}/first.md" "${TEST_TMP}/second.md"

  # Marker sanity: still exactly one generated section, and the
  # hand-written body survives.
  [ "$(grep -cx '## Release notes' "${TEST_TMP}/second.md")" -eq 1 ]
  [ "$(grep -cxF "${START_MARKER}" "${TEST_TMP}/second.md")" -eq 1 ]
  [ "$(grep -cxF "${END_MARKER}" "${TEST_TMP}/second.md")" -eq 1 ]
  grep -qF "Hand-written body text" "${TEST_TMP}/second.md"
}

@test "start marker without end marker keeps the full body" {
  run run_prepend "${FIXTURES_DIR}/release-notes-body-start-marker-no-end.md" \
    "${NOTES}" "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 0 ]

  write_expected "${NOTES}" \
    "${FIXTURES_DIR}/release-notes-body-start-marker-no-end.md" \
    "${TEST_TMP}/expected.md"
  diff -u "${TEST_TMP}/expected.md" "${TEST_TMP}/out.md"

  grep -qF "malformed previous run" "${TEST_TMP}/out.md"
  grep -qF "must never be lost" "${TEST_TMP}/out.md"
}

@test "body without markers is prepended" {
  printf 'Plain release body with no generated section.\n' \
    > "${TEST_TMP}/current.md"
  run run_prepend "${TEST_TMP}/current.md" "${NOTES}" "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 0 ]

  write_expected "${NOTES}" "${TEST_TMP}/current.md" "${TEST_TMP}/expected.md"
  diff -u "${TEST_TMP}/expected.md" "${TEST_TMP}/out.md"
}

@test "notes file containing a --- horizontal rule is accepted" {
  : > "${TEST_TMP}/current.md"
  run run_prepend "${TEST_TMP}/current.md" \
    "${FIXTURES_DIR}/release-notes-notes-with-horizontal-rule.md" \
    "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 0 ]

  # The rule cannot collide with the marker protocol, so the notes —
  # rule included — land verbatim in the merged body.
  [ "$(grep -cx -- '---' "${TEST_TMP}/out.md")" -eq 1 ]
  grep -qF "Trailing text after the rule." "${TEST_TMP}/out.md"

  write_expected "${FIXTURES_DIR}/release-notes-notes-with-horizontal-rule.md" \
    /dev/null "${TEST_TMP}/expected.md"
  diff -u "${TEST_TMP}/expected.md" "${TEST_TMP}/out.md"
}

@test "hand-written --- rules elsewhere in the body survive a re-run" {
  run run_prepend "${FIXTURES_DIR}/release-notes-body-with-section.md" \
    "${FIXTURES_DIR}/release-notes-notes-with-horizontal-rule.md" \
    "${TEST_TMP}/first.md"
  echo "$output"
  [ "$status" -eq 0 ]

  run run_prepend "${TEST_TMP}/first.md" "${NOTES}" "${TEST_TMP}/second.md"
  echo "$output"
  [ "$status" -eq 0 ]

  # The notes' rule was stripped with the generated section; the
  # hand-written body text is intact and markers stay singular.
  ! grep -qF "Trailing text after the rule." "${TEST_TMP}/second.md"
  grep -qF "Hand-written body text" "${TEST_TMP}/second.md"
  [ "$(grep -cxF "${START_MARKER}" "${TEST_TMP}/second.md")" -eq 1 ]
  [ "$(grep -cxF "${END_MARKER}" "${TEST_TMP}/second.md")" -eq 1 ]
}

@test "notes file containing a start marker is refused" {
  : > "${TEST_TMP}/current.md"
  run run_prepend "${TEST_TMP}/current.md" \
    "${FIXTURES_DIR}/release-notes-notes-with-start-marker.md" \
    "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 1 ]
  grep -qF "::error::" <<< "$output"
  # Refusal happens before emit: no output file is written.
  [ ! -e "${TEST_TMP}/out.md" ]
}

@test "notes file containing an end marker is refused" {
  : > "${TEST_TMP}/current.md"
  run run_prepend "${TEST_TMP}/current.md" \
    "${FIXTURES_DIR}/release-notes-notes-with-end-marker.md" \
    "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 1 ]
  grep -qF "::error::" <<< "$output"
  # Refusal happens before emit: no output file is written.
  [ ! -e "${TEST_TMP}/out.md" ]
}

@test "CRLF body with a previous generated section is replaced, not stacked" {
  run run_prepend "${FIXTURES_DIR}/release-notes-body-with-section-crlf.md" \
    "${NOTES}" "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 0 ]

  # Exactly one generated heading and one of each marker.
  [ "$(grep -cx '## Release notes' "${TEST_TMP}/out.md")" -eq 1 ]
  [ "$(grep -cxF "${START_MARKER}" "${TEST_TMP}/out.md")" -eq 1 ]
  [ "$(grep -cxF "${END_MARKER}" "${TEST_TMP}/out.md")" -eq 1 ]

  # The stale section is gone; the hand-written body survives, and no
  # CR survives into the merged body.
  ! grep -qF "Stale feature" "${TEST_TMP}/out.md"
  grep -qF "Hand-written body text" "${TEST_TMP}/out.md"
  ! grep -q $'\r' "${TEST_TMP}/out.md"

  # Byte-for-byte the same merge as the LF fixture.
  printf 'Hand-written body text that must survive every merge.\n' \
    > "${TEST_TMP}/kept.md"
  write_expected "${NOTES}" "${TEST_TMP}/kept.md" "${TEST_TMP}/expected.md"
  diff -u "${TEST_TMP}/expected.md" "${TEST_TMP}/out.md"

  # A second run on the merged body is byte-identical (fixpoint).
  run run_prepend "${TEST_TMP}/out.md" "${NOTES}" "${TEST_TMP}/second.md"
  echo "$output"
  [ "$status" -eq 0 ]
  diff -u "${TEST_TMP}/out.md" "${TEST_TMP}/second.md"
}

@test "missing input file is refused" {
  run run_prepend "${TEST_TMP}/does-not-exist.md" "${NOTES}" "${TEST_TMP}/out.md"
  echo "$output"
  [ "$status" -eq 1 ]
  grep -qF "::error::" <<< "$output"
}
