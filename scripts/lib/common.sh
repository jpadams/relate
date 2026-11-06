#!/usr/bin/env bash
# Shared helpers for repo scripts (disk telemetry, CI plumbing, file
# checksums). Source this file from a script that has resolved the repo
# root, e.g.:
#   REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
#   source "$REPO/scripts/lib/common.sh"
# The bodies are POSIX sh, so /bin/sh harnesses source it with `.` too
# (scripts/tests/test_install_sh.sh).
# Do not paste copies of these helpers into callers.

# Free megabytes on the / filesystem (POSIX df, megabyte units).
free_mb() {
  df -Pm / | awk 'NR == 2 { print $4 }'
}

# Append one preformatted message line to $GITHUB_STEP_SUMMARY when the
# variable is set (local runs have no summary file). The caller owns the
# message text and format; this helper owns the guard + append mechanics.
step_summary_append() {
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '%s\n' "$1" >> "$GITHUB_STEP_SUMMARY"
  fi
}

# Bare-hex SHA256 of a file: sha256sum where it exists (Linux, CI
# runners), shasum -a 256 elsewhere (macOS hosts).
# distribution/install.sh keeps its own inline resolution — it is a
# piped curl|sh installer and cannot source repo files.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}
