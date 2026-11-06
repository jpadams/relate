#!/usr/bin/env bash
# report-disk: end-of-job disk-headroom telemetry (docs/testing/ci-lanes.md).
# Appends the free-space-on-/ number to the step summary so the per-lane
# free-disk MIN_FREE_MB thresholds can be calibrated from real data instead
# of a guess. Runs even when the job failed (the step carries if: always()).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# free_mb + step_summary_append (shared helpers; do not paste copies here).
# shellcheck source=lib/common.sh
source "$REPO/scripts/lib/common.sh"

msg="disk-headroom: $(free_mb) MB free on / at job end"
echo "$msg"
step_summary_append "$msg"
