#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C.UTF-8

# ============================================================
# release-notes-slack.sh
#
# Slack-leg policy for .github/workflows/release-notes.yml
# (ADR-0124, ADR-0125 amendment 2). This script owns the whole
# leg: the skip decision, payload validation, and the POST.
#
# Leg INACTIVE (graceful skip, LABS-133) — logs a skip and
# exits 0, never a failure:
#   - PUBLISH != "true" (dry run)
#   - SLACK_WEBHOOK_URL unset/empty
#   When the leg is inactive AND the payload is also missing
#   or invalid, a ::warning:: is emitted (still exit 0): the
#   graceful-skip principle of ADR-0124 applied to the
#   artifact itself (ADR-0125 amendment 2).
#
# Leg ACTIVE — validates the payload and POSTs it:
#   - payload must exist, be non-empty, and be valid JSON
#     whose 'blocks' field is an array; otherwise ::error::
#     and exit 1 (the curl POST would otherwise fail
#     opaquely against Slack)
#   - the payload must not contain the FIREWORKS_API_KEY
#     value (prompt-injection exfiltration guard — the
#     payload is POSTed off-runner); otherwise ::error::
#     and exit 1. The secret value is never echoed.
#   - POSTs the payload file to the webhook with curl
#
# Usage: release-notes-slack.sh PAYLOAD_FILE
# Env var (optional): PUBLISH (default false)
# Env var (optional): SLACK_WEBHOOK_URL
# Env var (optional): FIREWORKS_API_KEY — when set, the
#   active leg refuses to POST a payload containing its value
# ============================================================

# --- Input validation ---
if [ "$#" -ne 1 ]; then
  echo "::error::Usage: release-notes-slack.sh PAYLOAD_FILE"
  exit 1
fi

PAYLOAD_FILE="$1"
: "${PUBLISH:=false}"

# --- Payload validation ---
payload_problem=""
if [ ! -s "$PAYLOAD_FILE" ]; then
  payload_problem="missing or empty"
elif ! jq -e '.blocks | type == "array"' "$PAYLOAD_FILE" > /dev/null; then
  payload_problem="not valid JSON with an array 'blocks' field"
fi

# --- Leg policy ---
skip_reason=""
if [ "${PUBLISH}" != "true" ]; then
  skip_reason="dry run (PUBLISH=${PUBLISH})"
elif [ -z "${SLACK_WEBHOOK_URL:-}" ]; then
  skip_reason="SLACK_WEBHOOK_URL secret not set"
fi

if [ -n "${skip_reason}" ]; then
  echo "[release-notes] skipping Slack post: ${skip_reason}"
  if [ -n "${payload_problem}" ]; then
    echo "::warning::Payload file '${PAYLOAD_FILE}' is ${payload_problem}; Slack leg inactive (${skip_reason}) — continuing"
  fi
  exit 0
fi

# --- Active leg: fail closed on a bad payload, then POST ---
if [ -n "${payload_problem}" ]; then
  echo "::error::Payload file '${PAYLOAD_FILE}' is ${payload_problem}"
  exit 1
fi
# Secret-content scan: refuse to POST a payload that contains the
# FIREWORKS_API_KEY value (fixed-string grep against the env var; the
# secret itself is never echoed). On the inactive leg this check is
# deliberately skipped — a payload that is never posted never leaves
# the runner, and the print step is masked by GitHub.
if [ -n "${FIREWORKS_API_KEY:-}" ] && grep -qF "${FIREWORKS_API_KEY}" "${PAYLOAD_FILE}"; then
  echo "::error::Payload file '${PAYLOAD_FILE}' contains the FIREWORKS_API_KEY value — refusing to post"
  exit 1
fi
curl -fsS -X POST \
  -H 'Content-Type: application/json' \
  --data @"$PAYLOAD_FILE" \
  "${SLACK_WEBHOOK_URL}"
echo "[release-notes] Slack payload posted"
