"""The markdown render half of the loadtest comparator.

Everything only render mode (`--render-only` / `--markdown-out`) needs:
the render formatters, the row builders, the per-kind section builders,
the per-use-case p99 details block, the mismatch explainer,
`render_markdown` itself, and the render-mode output writers. Pure
presentation: it consumes the shared verdict layer
(`compare_loadtest_verdict.py`) and never affects the
gate's diagnostics or exit code.

`compare-loadtest.py` (a hyphenated script Python cannot import as a
module) loads this underscore sibling with
`importlib.util.spec_from_file_location`, the established pattern for
hyphenated scripts under `scripts/`.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path
from typing import Callable, NamedTuple

_VERDICT = Path(__file__).resolve().parent / "compare_loadtest_verdict.py"
_spec = importlib.util.spec_from_file_location("compare_loadtest_verdict", _VERDICT)
_verdict = importlib.util.module_from_spec(_spec)
assert _spec.loader is not None
_spec.loader.exec_module(_verdict)

# The shared verdict layer, bound once: every band a row evaluates, every
# verdict it maps, and the per-kind policy the explainer projects from are
# defined there, exactly once.
BREACH = _verdict.BREACH
EnvMismatch = _verdict.EnvMismatch
MISSING = _verdict.MISSING
NEW = _verdict.NEW
NO_P99 = _verdict.NO_P99
NOT_GATED = _verdict.NOT_GATED
OK = _verdict.OK
PAIR_POLICIES = _verdict.PAIR_POLICIES
PairChecks = _verdict.PairChecks
Thresholds = _verdict.Thresholds
UsecaseVerdict = _verdict.UsecaseVerdict
errors_breach = _verdict.errors_breach
logical_breach = _verdict.logical_breach
ops_breach = _verdict.ops_breach
pair_checks = _verdict.pair_checks
resolve_report_kind = _verdict.resolve_report_kind
rss_breach = _verdict.rss_breach
shape_label = _verdict.shape_label
shape_logical_breach = _verdict.shape_logical_breach
shape_map = _verdict.shape_map
usecase_verdicts = _verdict.usecase_verdicts


# ---------------------------------------------------------------------------
# Markdown rendering (--render-only / --markdown-out).
#
# Pure presentation: consumes the shared verdict layer, never gates, and
# never affects the gate's diagnostics or exit code.
# ---------------------------------------------------------------------------

SKIP = "skipped (INCOMPATIBLE)"

# Render-mode pair-status tokens (--status-out), in precedence order.
STATUS_OK = "ok"
STATUS_FAILING = "failing-rows"
STATUS_NOT_COMPARABLE = "not-comparable"
STATUS_MISMATCH = "environment-mismatch"


def _mismatch_explainer(kind: str) -> str:
    """Render-only explainer appended directly under the environment-class
    mismatch blockquote: names the rows this pair kind devalues (the
    per-kind policy's devalued phrase), why, and the remedy (ADR-0071).
    Gate mode never prints it."""
    return (
        f"> {PAIR_POLICIES[kind].devalued}: the environment class changed "
        "since the baseline was frozen, so the runs are not comparable "
        "(ADR-0071). Re-freeze via the create-loadtest-baselines workflow "
        "to restore comparison."
    )


class Row(NamedTuple):
    """One rendered table row plus its failure flag. The flag is bound at
    the same expression that builds the `fail (…)` status cell, so the
    --status-out token (derived from the flags) can never drift from the
    table's rendered text."""

    cells: list[str]
    failed: bool = False


def _esc(cell: str) -> str:
    """Escape report-derived free text: the PR author controls the report
    contents, so every value that reaches the markdown — table cells and
    inline prose alike — passes through this once, at construction."""
    return (
        cell.replace("|", "\\|")
        .replace("\n", " ")
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
    )


def _fmt_int(n) -> str:
    return "—" if n is None else f"{n:,}"


def _fmt_ops(x) -> str:
    return "—" if x is None else f"{x:.2f}"


def _fmt_ms(ns) -> str:
    return "—" if ns is None else f"{ns / 1_000_000:.2f} ms"


def _fmt_ratio_value(r) -> str:
    return "—" if r is None else f"{r:.3f}"


def _delta(cur, base, mag: Callable[[object], str]) -> str:
    """Signed delta cell: `+mag (+pct%)`, percent only when base > 0."""
    if cur is None or base is None:
        return "—"
    diff = cur - base
    out = ("+" if diff >= 0 else "-") + mag(abs(diff))
    if base > 0:
        pct = (cur - base) / base * 100
        out += f" ({pct:+.1f}%)"
    return out


def _table(rows: list[Row], header: list[str] | None = None) -> list[str]:
    """One markdown table; `header` defaults to the comparison-table columns.

    Cells arrive pre-escaped: report-derived fragments (ids, environment
    classes, shape labels) pass through `_esc` where they are built, while
    the renderer-authored status vocabulary and fixed labels carry no pipe
    or newline — so nothing is escaped a second time here.
    """
    if header is None:
        header = ["Metric", "Baseline", "PR", "Δ", "Status"]
    lines = [
        "| " + " | ".join(header) + " |",
        "| " + " | ".join(["---"] * len(header)) + " |",
    ]
    lines += ["| " + " | ".join(row.cells) + " |" for row in rows]
    return lines


def _env_class(env) -> str:
    if not isinstance(env, dict):
        return "missing"
    return env.get("class") or "missing"


def _environment_row(base: dict, cur: dict, env_mismatches: list[EnvMismatch]) -> Row:
    """Environment-class row; the mismatch names come from the one walk."""
    if env_mismatches:
        status = f"mismatch ({', '.join(m.name for m in env_mismatches)})"
    else:
        status = "match"
    return Row(
        [
            "environment",
            _esc(_env_class(base.get("environment"))),
            _esc(_env_class(cur.get("environment"))),
            "—",
            status,
        ]
    )


def _banded(
    mismatch: bool, applicable: bool, evaluated: tuple[str, str, bool]
) -> tuple[str, str, bool]:
    """(delta, status, failed) for a perf band: devalued on an
    environment-class mismatch, `n/a` when there is no baseline value to
    compare against."""
    if mismatch:
        return "—", SKIP, False
    if not applicable:
        return "—", "n/a", False
    return evaluated


def _generic_rows(
    base: dict,
    cur: dict,
    th: Thresholds,
    env_mismatches: list[EnvMismatch],
    verdicts: list[UsecaseVerdict],
) -> list[Row]:
    """Fraud/identity shape: environment, errors, ops/s, p99, logical bytes."""
    mismatch = bool(env_mismatches)
    rows = [_environment_row(base, cur, env_mismatches)]

    b_err = sum(u.get("errors") or 0 for u in base.get("usecases") or [])
    c_err = sum(u.get("errors") or 0 for u in cur.get("usecases") or [])
    errors_failed = errors_breach(c_err)
    rows.append(
        Row(
            [
                "errors",
                _fmt_int(b_err),
                _fmt_int(c_err),
                _delta(c_err, b_err, str),
                f"fail ({c_err} errors)" if errors_failed else "ok",
            ],
            failed=errors_failed,
        )
    )

    b_tp = (base.get("throughput") or {}).get("ops_per_sec")
    c_tp = (cur.get("throughput") or {}).get("ops_per_sec")
    ops_failed = ops_breach(c_tp, b_tp, th)
    ops_delta, ops_status, ops_failed = _banded(
        mismatch,
        bool(b_tp),
        (
            _delta(c_tp, b_tp, _fmt_ops),
            f"fail (< {th.min_tp}× baseline)" if ops_failed else "ok",
            ops_failed,
        ),
    )
    rows.append(
        Row(
            ["ops/s", _fmt_ops(b_tp), _fmt_ops(c_tp), ops_delta, ops_status],
            failed=ops_failed,
        )
    )

    # Worst p99 regression: the use case furthest above its limit when any
    # breaches, else the one with the largest adverse delta.
    gated = [v for v in verdicts if v.status in (BREACH, OK)]
    if gated:
        breaches = [v for v in gated if v.status == BREACH]
        worst = (
            max(breaches, key=lambda v: v.excess)
            if breaches
            else max(gated, key=lambda v: v.cur_p99 - v.base_p99)
        )
        p99_failed = worst.status == BREACH
        p99_delta, p99_status, p99_failed = _banded(
            mismatch,
            True,  # a gated worst exists; the no-comparable case is the row below
            (
                _delta(worst.cur_p99, worst.base_p99, _fmt_ms),
                "fail (above limit)" if p99_failed else "ok",
                p99_failed,
            ),
        )
        rows.append(
            Row(
                [
                    f"p99 (worst: {_esc(worst.uid)})",
                    _fmt_ms(worst.base_p99),
                    _fmt_ms(worst.cur_p99),
                    p99_delta,
                    p99_status,
                ],
                failed=p99_failed,
            )
        )
    else:
        rows.append(
            Row(
                ["p99", "—", "—", "—", SKIP if mismatch else "n/a (no comparable use cases)"]
            )
        )

    b_mem = (base.get("memory") or {}).get("logical_used_bytes")
    c_mem = (cur.get("memory") or {}).get("logical_used_bytes")
    logical_failed = bool(b_mem) and logical_breach(c_mem, b_mem, th)
    rows.append(
        Row(
            [
                "logical bytes",
                _fmt_int(b_mem),
                _fmt_int(c_mem),
                _delta(c_mem, b_mem, _fmt_int),
                "n/a"
                if not b_mem
                else (f"fail (> {th.max_mem}× baseline)" if logical_failed else "ok"),
            ],
            failed=logical_failed,
        )
    )
    return rows


_P99_HEADER = ["Use case", "ops", "Baseline p99", "PR p99", "Δ", "Status"]


def _p99_rows(
    verdicts: list[UsecaseVerdict], th: Thresholds, env_mismatches: list[EnvMismatch]
) -> list[Row]:
    """Per-use-case p99 rows: the status ladder maps verdicts, with an
    environment mismatch devaluing comparable rows to `skipped`."""
    mismatch = bool(env_mismatches)
    rows = []
    for v in verdicts:
        if v.status == NEW:
            status, failed = "new (not in baseline)", False
        elif v.status == MISSING:
            status, failed = "fail (missing in PR)", True
        elif mismatch:
            status, failed = "skipped", False
        elif v.status == NO_P99:
            status, failed = "n/a (no p99)", False
        elif v.status == NOT_GATED:
            status, failed = f"not gated (ops < {th.min_ops_for_p99})", False
        elif v.status == BREACH:
            status, failed = "fail (above limit)", True
        else:
            status, failed = "ok", False
        delta = (
            "—"
            if v.status in (NEW, MISSING) or mismatch
            else _delta(v.cur_p99, v.base_p99, _fmt_ms)
        )
        rows.append(
            Row(
                [
                    _esc(v.uid),
                    _fmt_int(v.ops),
                    _fmt_ms(v.base_p99),
                    _fmt_ms(v.cur_p99),
                    delta,
                    status,
                ],
                failed=failed,
            )
        )
    return rows


def _p99_details(rows: list[Row]) -> list[str]:
    """Every use case's p99 row, collapsed behind a summary."""
    return [
        "<details>",
        "<summary>Per-use-case p99 latency</summary>",
        "",
        *_table(rows, header=_P99_HEADER),
        "",
        "</details>",
    ]


def _memory_rows(
    base: dict, cur: dict, th: Thresholds, env_mismatches: list[EnvMismatch]
) -> list[Row]:
    """Memory-shape pair: exact logical bytes and banded RSS ratio per shape."""
    mismatch = bool(env_mismatches)
    rows = [_environment_row(base, cur, env_mismatches)]
    b_shapes, c_shapes = shape_map(base), shape_map(cur)
    order = list(b_shapes) + [k for k in c_shapes if k not in b_shapes]
    for key in order:
        label = _esc(shape_label(key))
        b, c = b_shapes.get(key), c_shapes.get(key)
        if b is None or c is None:
            rows.append(
                Row(
                    [
                        f"logical bytes — {label}",
                        _fmt_int(None if b is None else b.get("logical_bytes")),
                        _fmt_int(None if c is None else c.get("logical_bytes")),
                        "—",
                        "fail (missing)",
                    ],
                    failed=True,
                )
            )
            continue
        b_logical, c_logical = b.get("logical_bytes"), c.get("logical_bytes")
        logical_failed = shape_logical_breach(c_logical, b_logical)
        rows.append(
            Row(
                [
                    f"logical bytes — {label}",
                    _fmt_int(b_logical),
                    _fmt_int(c_logical),
                    _delta(c_logical, b_logical, _fmt_int),
                    "fail (must be exact)" if logical_failed else "ok",
                ],
                failed=logical_failed,
            )
        )
        b_ratio, c_ratio = b.get("ratio"), c.get("ratio")
        rss_failed = rss_breach(c_ratio, b_ratio, th)
        rss_delta, rss_status, rss_failed = _banded(
            mismatch,
            bool(b_ratio and c_ratio),
            (
                _delta(c_ratio, b_ratio, _fmt_ratio_value),
                f"fail (> {th.max_rss_ratio}× baseline)" if rss_failed else "ok",
                rss_failed,
            ),
        )
        rows.append(
            Row(
                [
                    f"rss ratio — {label}",
                    _fmt_ratio_value(b_ratio),
                    _fmt_ratio_value(c_ratio),
                    rss_delta,
                    rss_status,
                ],
                failed=rss_failed,
            )
        )
    return rows


def _checkpoint_rows(base: dict, cur: dict, env_mismatches: list[EnvMismatch]) -> list[Row]:
    """Checkpoint pair: environment plus the top-level size fields (not gated)."""
    rows = [_environment_row(base, cur, env_mismatches)]
    for key in ("logical_used_bytes", "rss_bytes", "data_file_bytes"):
        b_v, c_v = base.get(key), cur.get(key)
        rows.append(
            Row([key, _fmt_int(b_v), _fmt_int(c_v), _delta(c_v, b_v, _fmt_int), "info"])
        )
    return rows


def _generic_section(
    base: dict, cur: dict, th: Thresholds, checks: PairChecks
) -> tuple[list[str], bool]:
    """The generic section: the comparison table plus the collapsed
    per-use-case p99 details; failing when either table has a failed row."""
    verdicts = usecase_verdicts(base, cur, th)
    rows = _generic_rows(base, cur, th, checks.env_mismatches, verdicts)
    detail_rows = _p99_rows(verdicts, th, checks.env_mismatches)
    lines = [*_table(rows), "", *_p99_details(detail_rows)]
    failing = any(row.failed for row in rows) or any(row.failed for row in detail_rows)
    return lines, failing


def _memory_section(
    base: dict, cur: dict, th: Thresholds, checks: PairChecks
) -> tuple[list[str], bool]:
    """The memory-shape section: the shape table; logical bytes stay banded
    on a mismatch, the rss rows are devalued by theirs."""
    rows = _memory_rows(base, cur, th, checks.env_mismatches)
    return _table(rows), any(row.failed for row in rows)


def _checkpoint_section(
    base: dict, cur: dict, th: Thresholds, checks: PairChecks
) -> tuple[list[str], bool]:
    """The checkpoint section: the size table. Size rows are informational
    only, so nothing fails; `th` is the uniform section shape — the
    checkpoint walk has no band."""
    rows = _checkpoint_rows(base, cur, checks.env_mismatches)
    return _table(rows), any(row.failed for row in rows)


# The renderer's per-kind projection, keyed by the resolve_report_kind
# vocabulary exactly as the gate keys PAIR_POLICIES: one dict lookup
# dispatches the section, no if/elif re-implementation of the kind split.
_SECTION_BUILDERS: dict[
    str, Callable[[dict, dict, Thresholds, PairChecks], tuple[list[str], bool]]
] = {
    "generic": _generic_section,
    "memory-shape": _memory_section,
    "checkpoint": _checkpoint_section,
}


def _status_token(failing: bool, checks: PairChecks) -> str:
    if failing:
        return STATUS_FAILING
    if checks.schema_msg is not None or checks.kind_msg is not None:
        return STATUS_NOT_COMPARABLE
    if checks.env_mismatches:
        return STATUS_MISMATCH
    return STATUS_OK


class Rendered(NamedTuple):
    """One rendered pair: the markdown document and its status token."""

    markdown: str
    status: str


def render_markdown(
    base: dict,
    cur: dict,
    *,
    base_path: Path,
    cur_path: Path,
    th: Thresholds,
    report_kind: str | None = None,
) -> Rendered:
    """One markdown comparison table for the pair plus its status token.
    The pair's kind selects its section builder from the module-level
    table — the render-side mirror of the gate's PAIR_POLICIES lookup.
    Pure: no printing."""
    checks = pair_checks(base, cur)
    kind = resolve_report_kind(base, cur, report_kind)

    lines = [f"### {base_path.stem or 'loadtest'}", "", f"`{base_path}` → `{cur_path}`", ""]

    if checks.schema_msg is not None:
        lines.append(f"> **Warning:** {_esc(checks.schema_msg)}.")
        lines.append("")
    if checks.kind_msg is not None:
        lines.append(f"> **Warning:** {_esc(checks.kind_msg)}.")
        lines.append("")
    if checks.env_messages:
        lines.append(
            "> **Environment class mismatch (INCOMPATIBLE):** "
            + _esc("; ".join(checks.env_messages))
        )
        lines.append(_mismatch_explainer(kind))
        lines.append("")

    section, failing = _SECTION_BUILDERS[kind](base, cur, th, checks)
    lines += section
    return Rendered("\n".join(lines) + "\n", _status_token(failing, checks))


def write_markdown(table: str, out: Path | None) -> None:
    if out is None or str(out) == "-":
        print(table, end="")
        return
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(table)


def write_status(token: str, out: Path) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(token + "\n")
