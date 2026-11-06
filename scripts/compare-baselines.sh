#!/usr/bin/env bash
# Compare frozen loadtest summaries to reports under this tree's target/.
#
# The pair set is declared ONCE, as the PAIR_REGISTRY below (one
# `file|kind` record per pair), and iterated by BOTH modes, so gate
# membership and render membership can never drift apart.
#
# Gate mode (default, no arguments): exit 1 when a pair fails or its
# current report is missing. The registry pairs are fraud and identity.
# Memory-shape and checkpoint are not registry members —
# the CI job runs and reports only the fraud and identity loadtests —
# though the comparator can still render their kinds when invoked directly.
#
# Render mode (`--render-only`): never gates, always exits 0. Aggregates
# every registry pair into ONE markdown document (default
# $REPORTS_DIR/loadtest-comparison.md): a `# Loadtest baseline comparison`
# heading, a producer-owned `**Status:** ok|warning` line (the
# authoritative overall verdict — consumers read it instead of grepping the
# `**Pairs:**` tokens), a `**Pairs:**` summary line with one `name: status`
# token per pair, then one `### <pair>` section per loadtest. A pair with
# baseline and report renders the comparator's per-pair table (wave 0's
# `--render-only --markdown-out -` contract), dispatched with the pair's
# registry `--report-kind` and classified from the comparator's
# `--status-out` token — never by grepping the rendered prose; a missing
# report renders a report-missing note; a missing baseline renders a
# not-compared-no-baseline-frozen note. The document is appended to
# $GITHUB_STEP_SUMMARY when set, printed to stdout otherwise.
#
# Env:
#   BASELINES       baseline directory (default <repo>/baselines)
#   REPORTS_DIR     current-report directory (default <repo>/target); render
#                   mode also writes the document here
#   COMPARISON_OUT  render-mode document path (default
#                   $REPORTS_DIR/loadtest-comparison.md)
#   PYTHON          python interpreter (default python3)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASELINES="${BASELINES:-$ROOT/baselines}"
REPORTS_DIR="${REPORTS_DIR:-$ROOT/target}"
COMPARE="${PYTHON:-python3} $ROOT/scripts/compare-loadtest.py"

# ---------------------------------------------------------------------------
# Pair registry — the ONE declaration of the loadtest pair set. Gate and
# render mode both iterate every record (in registry order, which is also
# the document order): every pair is gated, and a missing baseline still
# reaches the comparator and fails the gate.
#
# Record fields, `|`-separated:
#   1. file     report filename, resolved under BOTH $BASELINES and
#               $REPORTS_DIR (the pair's two sides share the name)
#   2. kind     comparator --report-kind dispatch for the pair's table
#               (render mode): generic | memory-shape | checkpoint — the
#               aggregator never relies on the comparator's content sniff
#      re-entry: a future pair needing a different membership or baseline
#      rule adds a field here, the one predicate that reads it, and its test
# ---------------------------------------------------------------------------
PAIR_REGISTRY=(
  "fraud-loadtest.json|generic"
  "identity-loadtest.json|generic"
)

if [[ $# -eq 0 ]]; then
  mode=gate
elif [[ $# -eq 1 && "${1:-}" == "--render-only" ]]; then
  mode=render
else
  echo "usage: compare-baselines.sh [--render-only]" >&2
  exit 2
fi

find_report() {
  local name="$1"
  if [[ -f "$REPORTS_DIR/$name" ]]; then
    echo "$REPORTS_DIR/$name"
  else
    echo ""
  fi
}

# Absolutize an input directory against the caller's cwd: an absolute path
# stays as-is, `./x` and `x` get the cwd prefixed (string prefixing, no
# `cd`: a missing directory is not an error). The result is printed, so
# callers assign it via command substitution — bash-3 has no namerefs.
resolve_dir() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    ./*) printf '%s\n' "$PWD/${1#./}" ;;
    *) printf '%s\n' "$PWD/$1" ;;
  esac
}

# Display path for the rendered document: repo-relative under $ROOT (nicer
# in a PR comment), absolute elsewhere.
display_path() {
  local p="$1"
  if [[ "$p" == "$ROOT"/* ]]; then
    printf '%s\n' "${p#"$ROOT"/}"
  else
    printf '%s\n' "$p"
  fi
}

# Neutralize non-authored text (comparator stderr, status tokens) before it
# reaches a markdown note: no unescaped & < > from the error text.
escape_note_text() {
  sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' <<<"$1"
}

# ---------------------------------------------------------------------------
# Render mode: one combined markdown document, never a gate.
# ---------------------------------------------------------------------------

render_tmp="" # global: the EXIT trap must see it after render_all returns

# Renders one pair's section into $render_tmp/<stem>.md and echoes the
# pair's token for the **Pairs:** summary line.
#
# Comparator tokens (--status-out) map onto the summary vocabulary:
#   ok                   → compared
#   failing-rows         → compared (failing rows)
#   environment-mismatch → compared (environment mismatch)
#   not-comparable       → compared (not comparable)
# The aggregator authors three tokens itself (no comparator run):
#   report-missing · not-compared-no-baseline-frozen · render-error.
# An unrecognized comparator token is contract drift: the pair is classed
# render-error and the script says so on stderr, so a comparator vocabulary
# change can never silently misclassify a pair as healthy.
render_pair() {
  local name="$1" kind="$2"
  local stem="${name%.json}"
  local base="$BASELINES/$name" cur="$REPORTS_DIR/$name"
  local sec="$render_tmp/$stem.md" status_file="$render_tmp/$stem.status"
  local base_disp cur_disp
  base_disp="$(display_path "$base")"
  cur_disp="$(display_path "$cur")"

  if [[ ! -f "$base" ]]; then
    cat >"$sec" <<EOF
### $stem

> **Note (not-compared-no-baseline-frozen):** no baseline at \`$base_disp\` — nothing is frozen to compare against yet.
EOF
    echo "not-compared-no-baseline-frozen"
    return
  fi
  if [[ ! -f "$cur" ]]; then
    cat >"$sec" <<EOF
### $stem

\`$base_disp\` → \`$cur_disp\`

> **Note (report-missing):** no current report at \`$cur_disp\` — the loadtest did not run.
EOF
    echo "report-missing"
    return
  fi

  # One table per pair (wave 0's render contract), captured from stdout.
  # --report-kind dispatches the table from the registry (no content
  # sniffing); --status-out carries the machine-readable pair verdict — the
  # summary token is read from it, never re-derived from the rendered
  # prose. Run from $ROOT so under-repo paths render repo-relative.
  local rc=0 table err token pair_status
  # shellcheck disable=SC2086
  table="$(cd "$ROOT" && $COMPARE --render-only --report-kind "$kind" \
    --markdown-out - --status-out "$status_file" "$base_disp" "$cur_disp" \
    2>"$render_tmp/$stem.err")" || rc=$?
  if [[ "$rc" -ne 0 ]]; then
    # Only unreadable/corrupt input JSON fails here. The gate reports it;
    # the document carries a note and rendering continues.
    err="$(tail -n 1 "$render_tmp/$stem.err")"
    echo "compare-baselines: render failed for $name (exit $rc)" >&2
    cat >"$sec" <<EOF
### $stem

\`$base_disp\` → \`$cur_disp\`

> **Warning (render-error):** comparator exited $rc: $(escape_note_text "${err:-no diagnostic}")
EOF
    echo "render-error"
    return
  fi
  token="$(cat "$status_file" 2>/dev/null || true)"
  case "$token" in
    ok) pair_status="compared" ;;
    failing-rows) pair_status="compared (failing rows)" ;;
    environment-mismatch) pair_status="compared (environment mismatch)" ;;
    not-comparable) pair_status="compared (not comparable)" ;;
    *)
      echo "compare-baselines: unexpected status token '$token' for $name" >&2
      cat >"$sec" <<EOF
### $stem

\`$base_disp\` → \`$cur_disp\`

> **Warning (render-error):** comparator exited 0 with unrecognized status token \`$(escape_note_text "$token")\`
EOF
      echo "render-error"
      return
      ;;
  esac
  printf '%s\n' "$table" >"$sec"
  echo "$pair_status"
}

# The document's overall status from the pair tokens — the producer-owned
# `**Status:**` line consumers read instead of grepping `**Pairs:**`:
#   ok      — every pair token is compared or not-compared-no-baseline-frozen
#   warning — any pair token is compared (failing rows), compared
#             (environment mismatch), compared (not comparable),
#             report-missing, or render-error; an unrecognized token also
#             escalates (fail loud, never silently healthy)
overall_status() {
  local tok overall=ok
  for tok in "$@"; do
    case "$tok" in
      compared|not-compared-no-baseline-frozen) ;; # ok-class tokens
      *) overall=warning ;;
    esac
  done
  printf '%s\n' "$overall"
}

render_all() {
  local out="${COMPARISON_OUT:-$REPORTS_DIR/loadtest-comparison.md}"

  render_tmp="$(mktemp -d)"
  trap 'rm -rf "$render_tmp"' EXIT

  local record name kind stem token entry line=""
  local -a sections=() summary=() pair_tokens=()
  for record in "${PAIR_REGISTRY[@]}"; do
    IFS='|' read -r name kind <<<"$record"
    stem="${name%.json}"
    token="$(render_pair "$name" "$kind")"
    sections+=("$render_tmp/$stem.md")
    summary+=("$stem: $token")
    pair_tokens+=("$token")
  done
  for entry in "${summary[@]}"; do
    if [[ -n "$line" ]]; then
      line+=$' · '
    fi
    line+="$entry"
  done
  local overall
  overall="$(overall_status "${pair_tokens[@]}")"

  mkdir -p "$(dirname "$out")"
  {
    printf '# Loadtest baseline comparison\n\n'
    # shellcheck disable=SC2016 # literal backticks: markdown code span
    printf 'Frozen baselines vs current reports; bands match the %s gate.\n\n' '`compare-loadtests`'
    printf '**Status:** %s\n\n' "$overall"
    printf '**Pairs:** %s\n\n' "$line"
    local sec
    for sec in "${sections[@]}"; do
      cat "$sec"
      printf '\n'
    done
  } >"$out"

  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    mkdir -p "$(dirname "$GITHUB_STEP_SUMMARY")"
    # A blank separator when the summary already carries content; separate
    # appends so the size check never shares a pipeline with the write.
    if [[ -s "$GITHUB_STEP_SUMMARY" ]]; then
      printf '\n' >>"$GITHUB_STEP_SUMMARY"
    fi
    cat "$out" >>"$GITHUB_STEP_SUMMARY"
  else
    cat "$out"
  fi
}

if [[ "$mode" == "render" ]]; then
  # The comparator runs from $ROOT with repo-relative display paths; resolve
  # relative input dirs against the startup cwd first.
  BASELINES="$(resolve_dir "$BASELINES")"
  REPORTS_DIR="$(resolve_dir "$REPORTS_DIR")"
  render_all
  exit 0
fi

# ---------------------------------------------------------------------------
# Gate mode: iterate every registry pair. Fraud and identity are
# unconditional.
# ---------------------------------------------------------------------------

fail=0
compare_one() {
  local name="$1"
  local cur
  cur="$(find_report "$name")"
  if [[ -z "$cur" ]]; then
    echo "compare-baselines: missing current report $name" >&2
    fail=1
    return
  fi
  # shellcheck disable=SC2086
  if ! $COMPARE "$BASELINES/$name" "$cur"; then
    fail=1
  fi
}

for record in "${PAIR_REGISTRY[@]}"; do
  IFS='|' read -r name _ <<<"$record"
  compare_one "$name"
done
exit "$fail"
