#!/usr/bin/env bash
#
# check-doc-examples.sh — execute the documented function examples for real.
#
# Extracts every self-contained RETURN/UNWIND example from the functions
# reference (docs/reference/functions.md) and runs each one through the
# Relate CLI against an in-memory database. The script fails (exit 1) if any
# documented example errors.
#
# This is what keeps the functions reference honest: if the docs claim a
# function/spelling the engine does not actually accept (a past vector
# function correction), or a future refactor renames/removes one, this check
# turns red.
#
# A second phase does the same for the EXPLAIN/PROFILE examples in the
# Cypher syntax reference (docs/reference/cypher-syntax.md), keeping the
# documented query-prefix output shape (issue #14) executable end to end.
#
# "Self-contained" means the example uses only literals — examples that
# reference graph variables (e.g. `RETURN id(n)`, `WHERE exists(n.age)`) are
# skipped, since they need a bound row. In practice the extractor picks up:
#   * every inline table/prose example annotated with an expected result
#     (`... → ...`), and
#   * every whole-line RETURN/UNWIND statement inside a ```cypher fenced block
#     (trailing `-- expected` comments are stripped).
#
# Usage:
#   scripts/check-doc-examples.sh [path/to/functions.md]
# Environment:
#   RELATE_BIN   explicit path to the relate binary (else auto-detects
#                    the ADR-0251 prebuilt dir for the current commit, then
#                    target/release/relate, then target/debug/relate).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="${1:-$REPO_ROOT/docs/reference/functions.md}"
SYNTAX_DOC="$REPO_ROOT/docs/reference/cypher-syntax.md"

if [[ ! -f "$DOC" ]]; then
  echo "error: doc file not found: $DOC" >&2
  exit 2
fi
if [[ ! -f "$SYNTAX_DOC" ]]; then
  echo "error: doc file not found: $SYNTAX_DOC" >&2
  exit 2
fi

# Locate the CLI binary: explicit override, then the per-SHA shared
# prebuilt dir (ADR-0251 §5, scripts/prebuilt-bin.sh), then release, then
# debug. The prebuilt probe matches only the current commit's directory
# (clean or dirty), never a stale SHA.
BIN="${RELATE_BIN:-}"
if [[ -z "$BIN" ]] && command -v git >/dev/null 2>&1; then
  _sha="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || true)"
  if [[ -n "$_sha" ]]; then
    for _cand in \
      "$REPO_ROOT/target/.prebuilt/$_sha/release/relate" \
      "$REPO_ROOT/target/.prebuilt/$_sha/debug/relate" \
      "$REPO_ROOT/target/.prebuilt/$_sha-dirty/release/relate" \
      "$REPO_ROOT/target/.prebuilt/$_sha-dirty/debug/relate"; do
      if [[ -x "$_cand" ]]; then
        BIN="$_cand"
        break
      fi
    done
  fi
fi
if [[ -z "$BIN" ]]; then
  if [[ -x "$REPO_ROOT/target/release/relate" ]]; then
    BIN="$REPO_ROOT/target/release/relate"
  elif [[ -x "$REPO_ROOT/target/debug/relate" ]]; then
    BIN="$REPO_ROOT/target/debug/relate"
  else
    echo "error: no relate binary found." >&2
    echo "       build one (cargo build -r -p relate-cli) or set RELATE_BIN." >&2
    exit 2
  fi
fi

# The example runs below are harness spawns, not product usage: opt every
# CLI child out of telemetry (ADR-0079 §6). Exported once so every $BIN
# invocation, present and future, carries it.
export DO_NOT_TRACK=1

echo "Doc-example check"
echo "  doc:    $DOC"
echo "  doc:    $SYNTAX_DOC (EXPLAIN/PROFILE prefixes)"
echo "  binary: $BIN"
echo

# --- Extract one query per record (NUL-delimited) using python3. -------------
# python3 is preinstalled on GitHub-hosted runners and available on every dev
# machine that builds this project; the extraction logic lives here rather than
# in a separate file to keep the checker a single script. Read with a NUL loop
# (portable to bash 3.2, e.g. stock macOS) rather than `mapfile`.
#
# $1: doc path; $2: comma-separated uppercase statement prefixes to collect.
extract_queries() {
  python3 - "$1" "$2" <<'PYEOF'
import re, sys

text = open(sys.argv[1], encoding="utf-8").read()
prefixes = tuple(p.strip() for p in sys.argv[2].split(","))

queries = []
in_fence = False
fence_lang = ""

def is_runnable(stmt):
    up = stmt.strip().upper()
    return any(up.startswith(p + " ") for p in prefixes)

for line in text.splitlines():
    stripped = line.strip()
    if stripped.startswith("```"):
        if not in_fence:
            in_fence = True
            fence_lang = stripped[3:].strip().lower()
        else:
            in_fence = False
            fence_lang = ""
        continue

    if in_fence:
        # Only Cypher fences hold runnable examples.
        if fence_lang != "cypher":
            continue
        # Drop a trailing "-- expected result" comment, then keep whole-line
        # statements with an allowed prefix (skips CALL ... YIELD, pure
        # comment lines).
        code = re.sub(r"--.*$", "", line).strip()
        if code and is_runnable(code):
            queries.append(code)
        continue

    # Outside fences: inline code spans annotated with an expected result,
    # i.e. `<query>` immediately followed by the → marker (tables and prose).
    for m in re.finditer(r"`([^`]+)`\s*→", line):
        q = m.group(1).strip()
        if is_runnable(q):
            queries.append(q)

# Un-escape markdown table escapes (\| pipe, \` backtick) and de-duplicate
# while preserving first-seen order.
seen = set()
out = []
for q in queries:
    q = q.replace("\\|", "|").replace("\\`", "`")
    if q not in seen:
        seen.add(q)
        out.append(q)

sys.stdout.write("\0".join(out))
if out:
    sys.stdout.write("\0")
PYEOF
}

QUERIES=()
while IFS= read -r -d '' q; do
  QUERIES+=("$q")
done < <(extract_queries "$DOC" "RETURN,UNWIND")

total=${#QUERIES[@]}
if (( total == 0 )); then
  echo "error: extracted 0 examples from $DOC — extractor or doc format changed." >&2
  exit 2
fi

# Next: the documented EXPLAIN/PROFILE prefix examples.
PREFIX_QUERIES=()
while IFS= read -r -d '' q; do
  PREFIX_QUERIES+=("$q")
done < <(extract_queries "$SYNTAX_DOC" "EXPLAIN,PROFILE")

prefix_total=${#PREFIX_QUERIES[@]}
if (( prefix_total < 2 )); then
  echo "error: extracted $prefix_total EXPLAIN/PROFILE example(s) from $SYNTAX_DOC — expected at least 2 (the documented EXPLAIN and PROFILE examples)." >&2
  exit 2
fi

failed=0
declare -a FAILURES=()

run_queries() {
  for q in "$@"; do
    [[ -z "$q" ]] && continue
    if out="$("$BIN" "$q" 2>&1)"; then
      printf '  ok    %s\n' "$q"
    else
      failed=$((failed + 1))
      FAILURES+=("$q"$'\n'"      -> $out")
      printf '  FAIL  %s\n' "$q"
    fi
  done
}

run_queries "${QUERIES[@]}"
run_queries "${PREFIX_QUERIES[@]}"

total=$((total + prefix_total))
echo
echo "Ran $total documented example(s); $failed failed."

if (( failed > 0 )); then
  echo
  echo "Failing examples (the doc claims something the engine rejects):" >&2
  for f in "${FAILURES[@]}"; do
    printf '  - %s\n' "$f" >&2
  done
  exit 1
fi

echo "All documented examples executed cleanly."
