# Behavioral tests for release-notes-slack.sh
#
# Locks the whole Slack-leg policy (LABS-133 / ADR-0124 / ADR-0125
# amendment 2): an inactive leg (dry run or unprovisioned webhook) logs a
# skip and exits 0 with no POST — additionally warning (still exit 0) when
# the payload is missing or invalid; an active leg validates the payload
# fail-closed (::error:: + exit 1 on missing/invalid), refuses to POST a
# payload containing the FIREWORKS_API_KEY value, and otherwise POSTs the
# payload file to the webhook.
#
# Local run:
#   bats .github/scripts/tests/release-notes-slack.bats
#
# Requirements:
#   bats-core >= 1.5.0  (brew install bats-core)

bats_require_minimum_version 1.5.0

SCRIPT="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)/release-notes-slack.sh"
FIXTURES_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")" && pwd)/fixtures"

PAYLOAD="${FIXTURES_DIR}/release-notes-slack-payload.json"

# ---------------------------------------------------------------------------
# setup / teardown
# ---------------------------------------------------------------------------

setup() {
  TEST_TMP="$(mktemp -d)"

  STUBS_DIR="${TEST_TMP}/stubs"
  mkdir -p "${STUBS_DIR}"

  # curl stub — logs its argv to CURL_LOG and exits 0. No network calls.
  cat >"${STUBS_DIR}/curl" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "${CURL_LOG:?CURL_LOG not set}"
exit 0
STUB
  chmod +x "${STUBS_DIR}/curl"

  CURL_LOG="${TEST_TMP}/curl.log"
  BAD_PAYLOAD="${TEST_TMP}/bad-payload.json"
  printf 'this is not json' > "${BAD_PAYLOAD}"
  export STUBS_DIR TEST_TMP CURL_LOG BAD_PAYLOAD
}

teardown() {
  rm -rf "${TEST_TMP}"
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Run the slack script with curl stubbed on PATH. First arg is the payload
# file; remaining args are env vars in NAME=VALUE form applied via env(1).
run_slack() {
  local payload="$1"
  shift
  env PATH="${STUBS_DIR}:${PATH}" "$@" bash "${SCRIPT}" "${payload}" 2>&1
}

# ---------------------------------------------------------------------------
# Tests: inactive leg (skip policy)
# ---------------------------------------------------------------------------

@test "dry run (PUBLISH unset) skips with exit 0 and no curl" {
  run run_slack "${PAYLOAD}"
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "skipping Slack post: dry run" <<< "$output"
  [ ! -f "${CURL_LOG}" ]
}

@test "dry run (PUBLISH=false) skips with exit 0 and no curl" {
  run run_slack "${PAYLOAD}" PUBLISH=false SLACK_WEBHOOK_URL="https://hooks.slack.invalid/x"
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "skipping Slack post: dry run (PUBLISH=false)" <<< "$output"
  [ ! -f "${CURL_LOG}" ]
}

@test "publish with unset webhook skips with exit 0 and no curl" {
  run run_slack "${PAYLOAD}" PUBLISH=true
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "skipping Slack post: SLACK_WEBHOOK_URL secret not set" <<< "$output"
  [ ! -f "${CURL_LOG}" ]
}

@test "publish with empty webhook skips with exit 0 and no curl" {
  run run_slack "${PAYLOAD}" PUBLISH=true SLACK_WEBHOOK_URL=""
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "skipping Slack post: SLACK_WEBHOOK_URL secret not set" <<< "$output"
  [ ! -f "${CURL_LOG}" ]
}

@test "inactive leg with invalid payload warns and exits 0 with no curl" {
  run run_slack "${BAD_PAYLOAD}" PUBLISH=false SLACK_WEBHOOK_URL="https://hooks.slack.invalid/x"
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "skipping Slack post: dry run" <<< "$output"
  grep -qF "::warning::" <<< "$output"
  grep -qF "not valid JSON with an array 'blocks' field" <<< "$output"
  [[ "$output" != *"::error::"* ]]
  [ ! -f "${CURL_LOG}" ]
}

@test "inactive leg with missing payload warns and exits 0 with no curl" {
  run run_slack "${TEST_TMP}/nonexistent.json" PUBLISH=true
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "skipping Slack post: SLACK_WEBHOOK_URL secret not set" <<< "$output"
  grep -qF "::warning::" <<< "$output"
  grep -qF "missing or empty" <<< "$output"
  [ ! -f "${CURL_LOG}" ]
}

# ---------------------------------------------------------------------------
# Tests: active leg
# ---------------------------------------------------------------------------

@test "active leg with invalid payload fails with exit 1 and no curl" {
  run run_slack "${BAD_PAYLOAD}" PUBLISH=true SLACK_WEBHOOK_URL="https://hooks.slack.invalid/x"
  echo "$output"
  [ "$status" -eq 1 ]
  grep -qF "::error::" <<< "$output"
  grep -qF "not valid JSON with an array 'blocks' field" <<< "$output"
  [ ! -f "${CURL_LOG}" ]
}

@test "active leg with missing payload fails with exit 1 and no curl" {
  run run_slack "${TEST_TMP}/nonexistent.json" PUBLISH=true SLACK_WEBHOOK_URL="https://hooks.slack.invalid/x"
  echo "$output"
  [ "$status" -eq 1 ]
  grep -qF "::error::" <<< "$output"
  grep -qF "missing or empty" <<< "$output"
  [ ! -f "${CURL_LOG}" ]
}

@test "publish with webhook POSTs the payload file via curl" {
  run run_slack "${PAYLOAD}" PUBLISH=true SLACK_WEBHOOK_URL="https://hooks.slack.invalid/services/T/B/x"
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "Slack payload posted" <<< "$output"

  [ -f "${CURL_LOG}" ]
  [ "$(wc -l < "${CURL_LOG}" | tr -d ' ')" -eq 1 ]
  grep -qF -- "-X POST" "${CURL_LOG}"
  grep -qF -- "-H Content-Type: application/json" "${CURL_LOG}"
  grep -qF -- "--data @${PAYLOAD}" "${CURL_LOG}"
  grep -qF "https://hooks.slack.invalid/services/T/B/x" "${CURL_LOG}"
}

# ---------------------------------------------------------------------------
# Tests: secret-content scan (active leg only)
# ---------------------------------------------------------------------------

@test "active leg refuses to POST a payload containing FIREWORKS_API_KEY" {
  SECRET_PAYLOAD="${TEST_TMP}/secret-payload.json"
  jq '.text = "leaked fw_test_secret_123"' "${PAYLOAD}" > "${SECRET_PAYLOAD}"

  run run_slack "${SECRET_PAYLOAD}" PUBLISH=true \
    SLACK_WEBHOOK_URL="https://hooks.slack.invalid/x" \
    FIREWORKS_API_KEY="fw_test_secret_123"
  echo "$output"
  [ "$status" -eq 1 ]
  grep -qF "::error::" <<< "$output"
  grep -qF "refusing to post" <<< "$output"
  # The secret value itself is never echoed.
  [[ "$output" != *"fw_test_secret_123"* ]]
  # curl was never invoked — nothing left the runner.
  [ ! -f "${CURL_LOG}" ]
}

@test "inactive leg with secret in payload still skips (never posted)" {
  SECRET_PAYLOAD="${TEST_TMP}/secret-payload.json"
  jq '.text = "leaked fw_test_secret_123"' "${PAYLOAD}" > "${SECRET_PAYLOAD}"

  run run_slack "${SECRET_PAYLOAD}" PUBLISH=false \
    FIREWORKS_API_KEY="fw_test_secret_123"
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "skipping Slack post: dry run" <<< "$output"
  [ ! -f "${CURL_LOG}" ]
}

@test "active leg POSTs normally when FIREWORKS_API_KEY is unset" {
  run run_slack "${PAYLOAD}" PUBLISH=true SLACK_WEBHOOK_URL="https://hooks.slack.invalid/x"
  echo "$output"
  [ "$status" -eq 0 ]
  grep -qF "Slack payload posted" <<< "$output"
  [ -f "${CURL_LOG}" ]
}
