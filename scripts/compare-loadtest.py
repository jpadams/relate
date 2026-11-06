#!/usr/bin/env python3
"""Compare a loadtest summary JSON against a frozen baseline.

Fails if errors > 0, throughput is below ratio, p99 latency is above
ratio, or logical_used_bytes exceeds ratio. Ratios are env-overridable.

Schema-aware: when both reports carry `schema_version` (report schema 6+),
they must be equal — a frozen baseline is meaningless across schema bumps.
Schema 6 nests workload-specific fields under `workload`; the gated fields
(`usecases`, `throughput`, `memory.logical_used_bytes`) stay top-level.

ADR-0071: wall-clock ops/s and p99 run only when `environment` classes
match. A mismatch is INCOMPATIBLE (hard fail in GitHub Actions; local
skips perf but still checks errors and logical bytes).

Markdown rendering: `--markdown-out FILE` writes one baseline-vs-PR
comparison table (per-row status, `<details>` per-use-case p99 breakdown)
without touching the gate. `--render-only` suppresses gating entirely —
no FAIL/INCOMPATIBLE diagnostics, exit code always 0 — so CI can render
tables for pairs the gate would fail or skip. Render mode also takes
`--status-out FILE` (one machine-readable pair-status token) and
`--report-kind` (dispatch the table without sniffing the reports).

Every verdict — the per-use-case p99 ladder, each gate band, the pair
prelude — is computed exactly once in the verdict layer below; the gate's
die lines and the markdown rows are both consumers of it, never parallel
implementations.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import sys
from pathlib import Path

# The shared verdict layer (bands, verdicts, diagnostics, per-kind
# policies) and the render half live in underscore siblings — hyphenated
# scripts cannot be imported as modules — each loaded with the established
# importlib.util.spec_from_file_location pattern (as loadtest_env.py is).
_VERDICT = Path(__file__).resolve().parent / "compare_loadtest_verdict.py"
_vspec = importlib.util.spec_from_file_location("compare_loadtest_verdict", _VERDICT)
_verdict = importlib.util.module_from_spec(_vspec)
assert _vspec.loader is not None
_vspec.loader.exec_module(_verdict)
Diag = _verdict.Diag
FAILED = _verdict.FAILED
INCOMPATIBLE = _verdict.INCOMPATIBLE
PAIR_POLICIES = _verdict.PAIR_POLICIES
Thresholds = _verdict.Thresholds
errors_breach = _verdict.errors_breach
pair_checks = _verdict.pair_checks
resolve_report_kind = _verdict.resolve_report_kind

_RENDER = Path(__file__).resolve().parent / "compare_loadtest_render.py"
_rspec = importlib.util.spec_from_file_location("compare_loadtest_render", _RENDER)
_render = importlib.util.module_from_spec(_rspec)

# The shared environment-class model (scripts/loadtest_env.py): the
# canonical in_github_actions lives there.
_ESPEC = importlib.util.spec_from_file_location("loadtest_env", Path(__file__).resolve().parent / "loadtest_env.py")
_env = importlib.util.module_from_spec(_ESPEC)
_ESPEC.loader.exec_module(_env)  # type: ignore[union-attr]
assert _rspec.loader is not None
_rspec.loader.exec_module(_render)
render_markdown = _render.render_markdown
write_markdown = _render.write_markdown
write_status = _render.write_status


def ratio(name: str, default: float) -> float:
    raw = os.environ.get(name)
    return float(raw) if raw else default


def load_thresholds() -> Thresholds:
    return Thresholds(
        min_tp=ratio("LOADTEST_MIN_THROUGHPUT_RATIO", 0.90),
        max_p99=ratio("LOADTEST_MAX_P99_RATIO", 1.15),
        max_mem=ratio("LOADTEST_MAX_LOGICAL_BYTES_RATIO", 1.10),
        p99_jitter_ns=int(os.environ.get("LOADTEST_P99_JITTER_NS", "250000")),
        max_rss_ratio=ratio("LOADTEST_MAX_RSS_RATIO", 1.15),
        min_ops_for_p99=int(os.environ.get("LOADTEST_MIN_OPS_FOR_P99", "8")),
    )


# ---------------------------------------------------------------------------
# The ADR-0071 merge gate: diagnostics and exit code, no rendering.
# ---------------------------------------------------------------------------

# The canonical CI detection lives in loadtest_env.py (in_github_actions),
# loaded here with the established importlib pattern — the local copy is
# deleted and the calls below use the shared module's.


def _hard(diag: Diag) -> bool:
    """A diagnostic fails the gate when it is a FAIL, or an INCOMPATIBLE
    inside GitHub Actions (ADR-0071: locally a mismatch only devalues the
    perf bands instead of failing the pair)."""
    return diag.level == FAILED or (diag.level == INCOMPATIBLE and _env.in_github_actions())


def finish(diags: list[Diag]) -> int:
    """The single exit tail. The verdict derives from the diagnostics
    themselves: any hard diagnostic fails the gate with the true count; a
    pair whose diagnostics are all INCOMPATIBLE (only reachable locally,
    where a mismatch devalues the perf bands) passes correctness-only."""
    for d in diags:
        print(f"compare-loadtest: {d.level}: {d.message}", file=sys.stderr)
    mismatch = next((d for d in diags if d.level == INCOMPATIBLE), None)
    if mismatch is not None and not _env.in_github_actions():
        print(f"compare-loadtest: {mismatch.note}", file=sys.stderr)
    if any(_hard(d) for d in diags):
        print(f"compare-loadtest: {len(diags)} check(s) failed", file=sys.stderr)
        return 1
    if diags:
        print("compare-loadtest: ok (correctness only)")
        return 0
    print("compare-loadtest: ok")
    return 0


def gate(base: dict, cur: dict, th: Thresholds) -> int:
    """The ADR-0071 merge gate: diagnostics and exit code, no rendering.
    The pair's kind — the resolve_report_kind vocabulary, always sniffed
    (`--report-kind` is a render-only flag) — selects its policy record
    from the one per-kind declaration; the correctness walk runs before
    the perf band, so every logical-bytes diagnostic keeps its place ahead
    of every ratio diagnostic."""
    diags: list[Diag] = []
    checks = pair_checks(base, cur)
    if checks.schema_msg is not None:
        diags.append(Diag.fail(checks.schema_msg))
    if checks.kind_msg is not None:
        diags.append(Diag.fail(checks.kind_msg))
    for u in cur.get("usecases") or []:
        err = u.get("errors") or 0
        if errors_breach(err):
            diags.append(Diag.fail(f"current usecase {u.get('id')}: errors={err}"))

    policy = PAIR_POLICIES[resolve_report_kind(base, cur, None)]
    policy.correctness(base, cur, th, diags)
    if checks.env_mismatches:
        diags.append(
            Diag.mismatch("; ".join(checks.env_messages), policy.skip_note)
        )
        return finish(diags)
    policy.perf(base, cur, th, diags)
    return finish(diags)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("baseline", type=Path)
    p.add_argument("current", type=Path)
    p.add_argument(
        "--render-only",
        action="store_true",
        help="render the markdown comparison without gating: no FAIL/INCOMPATIBLE "
        "diagnostics, exit code always 0",
    )
    p.add_argument(
        "--markdown-out",
        type=Path,
        default=None,
        metavar="FILE",
        help="write the markdown comparison table to FILE ('-' for stdout; with "
        "--render-only and no FILE the table goes to stdout)",
    )
    p.add_argument(
        "--status-out",
        type=Path,
        default=None,
        metavar="FILE",
        help="with --render-only: write the pair's machine-readable status token "
        "(ok | failing-rows | not-comparable | environment-mismatch) to FILE; "
        "ignored in gate mode",
    )
    p.add_argument(
        "--report-kind",
        choices=("generic", "memory-shape", "checkpoint"),
        default=None,
        help="with --render-only: render the table for this report kind (generic, "
        "memory-shape, checkpoint) instead of sniffing the report contents; "
        "ignored in gate mode",
    )
    args = p.parse_args()

    base = json.loads(args.baseline.read_text())
    cur = json.loads(args.current.read_text())
    th = load_thresholds()

    rc = 0 if args.render_only else gate(base, cur, th)
    if args.render_only or args.markdown_out is not None:
        rendered = render_markdown(
            base,
            cur,
            base_path=args.baseline,
            cur_path=args.current,
            th=th,
            report_kind=args.report_kind,
        )
        write_markdown(rendered.markdown, args.markdown_out)
        if args.render_only and args.status_out is not None:
            write_status(rendered.status, args.status_out)
    return rc


if __name__ == "__main__":
    sys.exit(main())
