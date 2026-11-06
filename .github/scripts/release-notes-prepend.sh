#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C.UTF-8

# ============================================================
# release-notes-prepend.sh
#
# Merge policy for the idempotent GitHub release update in
# .github/workflows/release-notes.yml (ADR-0124, amended by
# ADR-0125): prepend a fresh `## Release notes` section to the
# current release body.
#
# Pure text policy: current-body file + notes file in, merged
# body file out. The workflow keeps the `gh release view` /
# `gh release edit` orchestration around it.
#
# Generated-section protocol (ADR-0125 amendment 1): the ENTIRE
# generated block — heading included — is delimited by invisible
# HTML comment markers, which cannot collide with human- or
# agent-authored markdown:
#
#   <!-- release-notes:start -->
#   ## Release notes
#
#   <notes content>
#
#   <!-- release-notes:end -->
#
# The start marker is the first line of the block and the end
# marker the last; the hand-written body follows the end-marker
# line directly (no trailing blank separator). GitHub renders the
# HTML comments invisibly, so the visible body is unchanged:
# heading, notes, body.
#
# Prepend policy:
#   - CR characters are stripped from the current body before any
#     marker handling, so a CRLF body cannot defeat the exact-line
#     marker match and silently degrade replace into stacking
#     (same convention as enforce-opencode-verdict.sh).
#   - The notes file is refused when it contains either marker
#     string: the notes land in the generated block verbatim, so
#     a marker of their own would strand stale generated content
#     as "hand-written" body on the next re-run.
#   - If the current body contains a start marker followed by
#     an end marker, strip the previous generated block by
#     deleting from the start-marker line through the end-marker
#     line (inclusive) — nothing else — before prepending the new
#     block, so re-runs REPLACE rather than stack. Emit writes
#     `block ++ body` and strip removes exactly `block`, so two
#     consecutive runs are a byte fixpoint by construction.
#   - If a start marker is present WITHOUT an end marker, keep
#     the full body so we never lose data.
#
# Usage: release-notes-prepend.sh CURRENT_BODY_FILE NOTES_FILE OUTPUT_FILE
# ============================================================

HEADING='## Release notes'
START_MARKER='<!-- release-notes:start -->'
END_MARKER='<!-- release-notes:end -->'

# --- Input validation ---
if [ "$#" -ne 3 ]; then
  echo "::error::Usage: release-notes-prepend.sh CURRENT_BODY_FILE NOTES_FILE OUTPUT_FILE"
  exit 1
fi

CURRENT_BODY_FILE="$1"
NOTES_FILE="$2"
OUTPUT_FILE="$3"

for file in "$CURRENT_BODY_FILE" "$NOTES_FILE"; do
  if [ ! -f "$file" ]; then
    echo "::error::Input file '${file}' does not exist"
    exit 1
  fi
done

if [ ! -s "$NOTES_FILE" ]; then
  echo "::error::Notes file '${NOTES_FILE}' is empty"
  exit 1
fi

# --- Marker-content guard on the notes ---
# The notes land in the generated section verbatim. If they carried a
# marker string of their own, the next re-run would treat the stale
# generated content below it as hand-written body and strand it, so
# refuse instead of merging.
if grep -qF -e "$START_MARKER" -e "$END_MARKER" "$NOTES_FILE"; then
  echo "::error::Notes file '${NOTES_FILE}' contains a release-notes marker; refusing to merge"
  exit 1
fi

# --- Normalize line endings ---
# A CRLF body would defeat the exact-line marker match below (grep -x /
# awk string equality) and silently degrade replace into stacking, so
# strip CR characters before any marker handling (same convention as
# enforce-opencode-verdict.sh).
BODY_FILE="$(mktemp)"
trap 'rm -f "$BODY_FILE"' EXIT
tr -d '\r' < "$CURRENT_BODY_FILE" > "$BODY_FILE"

# --- Decide whether to strip a previous generated section ---
# Strip only when a start marker is followed by an end marker;
# a start marker without an end marker keeps the full body so
# hand-written content is never lost.
strip_from=""
strip_to=""
start_line="$(grep -nxF -e "$START_MARKER" "$BODY_FILE" | head -n 1 | cut -d: -f1 || true)"
if [ -n "$start_line" ]; then
  end_line="$(awk -v start="$start_line" -v end="$END_MARKER" \
    'NR > start && $0 == end { print NR; exit }' "$BODY_FILE")"
  if [ -n "$end_line" ]; then
    # The markers fully delimit the generated block (heading
    # included), so the strip is a single range delete: start-marker
    # line through end-marker line, inclusive. Nothing else.
    strip_from="$start_line"
    strip_to="$end_line"
  fi
fi

# --- Emit merged body ---
{
  printf '%s\n%s\n\n' "$START_MARKER" "$HEADING"
  cat "$NOTES_FILE"
  printf '\n%s\n' "$END_MARKER"
  if [ -n "$strip_from" ]; then
    awk -v a="$strip_from" -v b="$strip_to" 'NR < a || NR > b' "$BODY_FILE"
  else
    cat "$BODY_FILE"
  fi
} > "$OUTPUT_FILE"

echo "[release-notes] merged body written to ${OUTPUT_FILE} ($(wc -c < "$OUTPUT_FILE") bytes)"
