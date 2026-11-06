"""The shared comparison layer for the loadtest comparator: every
verdict, band, diagnostic, and per-kind policy, defined exactly once.

Both of the comparator's modes consume this module: the ADR-0071 gate and
CLI (`compare-loadtest.py`) and the markdown renderer
(`compare_loadtest_render.py`) each load this underscore sibling with
`importlib.util.spec_from_file_location` — the established pattern for
hyphenated scripts under `scripts/` (as with `loadtest_env.py`). The
gate's die lines and the markdown rows are both consumers of this layer,
never parallel implementations.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path
from typing import Callable, NamedTuple

_ENV = Path(__file__).resolve().parent / "loadtest_env.py"
_spec = importlib.util.spec_from_file_location("loadtest_env", _ENV)
_env = importlib.util.module_from_spec(_spec)
assert _spec.loader is not None
_spec.loader.exec_module(_env)
EnvMismatch = _env.EnvMismatch
environment_mismatches = _env.environment_mismatches


class Thresholds(NamedTuple):
    """Gate bands, shared by the gate and the markdown renderer."""

    min_tp: float
    max_p99: float
    max_mem: float
    p99_jitter_ns: int
    max_rss_ratio: float
    min_ops_for_p99: int


def p99_limit_ns(base_p99: int | float | None, th: Thresholds) -> int | float | None:
    """Worst PR p99 one use case may reach: ratio ceiling or baseline + jitter."""
    return max(th.max_p99 * base_p99, base_p99 + th.p99_jitter_ns) if base_p99 else None


def usecases(report: dict) -> dict:
    return {u["id"]: u for u in report.get("usecases") or []}


def shape_map(report: dict) -> dict:
    return {(s.get("shape"), s.get("index")): s for s in report.get("shapes") or []}


# ---------------------------------------------------------------------------
# Verdict layer. Every gate band and ladder is defined exactly once here;
# the gate's die lines, the markdown row builders, and the render-mode
# status token all consume these and never re-implement a comparison.
# ---------------------------------------------------------------------------

# Per-use-case p99 verdict statuses, in evaluation order.
NEW = "new"              # use case only in the PR (nothing to compare against)
MISSING = "missing"      # baseline use case absent from the PR
NO_P99 = "no-p99"        # p99 missing (or zero) on one side
NOT_GATED = "not-gated"  # measured ops below th.min_ops_for_p99
BREACH = "breach"        # gated and above the p99 limit
OK = "ok"                # gated and within the p99 limit


class UsecaseVerdict(NamedTuple):
    """One use case's p99 verdict plus the values its consumers render."""

    uid: str
    status: str
    base_p99: int | float | None
    cur_p99: int | float | None
    ops: int | None  # raw PR ops count; None when the use case is missing
    excess: int | float  # cur_p99 - limit when gated (BREACH/OK); else 0


def usecase_verdict(uid: str, b: dict | None, c: dict | None, th: Thresholds) -> UsecaseVerdict:
    """The one per-use-case p99 evaluation: the gate's die lines, the
    renderer's worst-p99 pick, and the per-use-case detail statuses all map
    from this verdict, so gate and tables cannot drift apart."""
    bp99 = (b.get("latency_ns") or {}).get("p99") if b else None
    cp99 = (c.get("latency_ns") or {}).get("p99") if c else None
    ops = c.get("ops") if c else None
    if b is None:
        return UsecaseVerdict(uid, NEW, None, cp99, ops, 0)
    if c is None:
        return UsecaseVerdict(uid, MISSING, bp99, None, None, 0)
    if not (bp99 and cp99):
        return UsecaseVerdict(uid, NO_P99, bp99, cp99, ops, 0)
    if (ops or 0) < th.min_ops_for_p99:
        return UsecaseVerdict(uid, NOT_GATED, bp99, cp99, ops, 0)
    limit = p99_limit_ns(bp99, th)
    excess = cp99 - limit
    return UsecaseVerdict(uid, BREACH if cp99 > limit else OK, bp99, cp99, ops, excess)


def usecase_verdicts(base: dict, cur: dict, th: Thresholds) -> list[UsecaseVerdict]:
    """Verdicts for every use case on either side, baseline order first."""
    bu, cu = usecases(base), usecases(cur)
    order = list(bu) + [uid for uid in cu if uid not in bu]
    return [usecase_verdict(uid, bu.get(uid), cu.get(uid), th) for uid in order]


# Gate bands. Each predicate is defined once; the gate turns a breach into
# a FAIL diagnostic, the row builders into a `fail (…)` status cell and its
# Row.failed flag.
def errors_breach(errors: int) -> bool:
    """Any errors at all is an absolute failure (ADR-0046)."""
    return errors != 0


def ops_breach(cur_tp, base_tp, th: Thresholds) -> bool:
    """ops/s below the ratio floor; no baseline throughput means no band."""
    return (base_tp or 0) > 0 and (cur_tp or 0.0) < th.min_tp * base_tp


def logical_breach(cur_mem, base_mem, th: Thresholds) -> bool:
    """logical_used_bytes above the ratio ceiling (checked even on mismatch)."""
    return (base_mem or 0) > 0 and (cur_mem or 0) > th.max_mem * base_mem


def shape_logical_breach(cur_logical, base_logical) -> bool:
    """Memory-shape logical bytes must match the baseline exactly."""
    return cur_logical != base_logical


def rss_breach(cur_ratio, base_ratio, th: Thresholds) -> bool:
    """RSS/logical ratio above the ratio ceiling; falsy ratios are n/a."""
    return bool(base_ratio) and bool(cur_ratio) and cur_ratio > th.max_rss_ratio * base_ratio


def workload_kind(report: dict):
    return (report.get("workload") or {}).get("kind")


def shape_label(key) -> str:
    """`shape/index` label fragment, shared by the gate and the rows."""
    return f"{key[0]}/{key[1]}"


class PairChecks(NamedTuple):
    """Pair-level prelude computed once, consumed by gate and renderer."""

    schema_msg: str | None  # die-line core when schema_version differs
    kind_msg: str | None  # die-line core when workload.kind differs
    env_mismatches: list[EnvMismatch]  # the one structured environment walk
    env_messages: list[str]  # one message per mismatch


def pair_checks(base: dict, cur: dict) -> PairChecks:
    """Schema, workload-kind, and environment verdicts for the pair."""
    b_schema, c_schema = base.get("schema_version"), cur.get("schema_version")
    b_kind, c_kind = workload_kind(base), workload_kind(cur)
    schema_msg = (
        f"schema_version {c_schema} != baseline {b_schema} (regenerate the baseline)"
        if b_schema is not None and c_schema is not None and b_schema != c_schema
        else None
    )
    kind_msg = (
        f"workload.kind {c_kind!r} != baseline {b_kind!r}"
        if b_kind is not None and c_kind is not None and b_kind != c_kind
        else None
    )
    env_mismatches = environment_mismatches(
        base.get("environment"), cur.get("environment")
    )
    return PairChecks(
        schema_msg=schema_msg,
        kind_msg=kind_msg,
        env_mismatches=env_mismatches,
        env_messages=[m.message for m in env_mismatches],
    )


# ---------------------------------------------------------------------------
# The gate protocol: diagnostics and the per-kind walks the gate runs.
# The gate driver (exit tail, the GHA/local distinction) lives in
# compare-loadtest.py; rendering lives in compare_loadtest_render.py.
# ---------------------------------------------------------------------------

FAILED = "FAIL"
INCOMPATIBLE = "INCOMPATIBLE"


class Diag(NamedTuple):
    """One gate diagnostic, built through one of its two constructors:
    `Diag.fail(message)` — a FAIL, which carries no skip line — or
    `Diag.mismatch(message, note)` — an INCOMPATIBLE, whose `note` (the
    skip line naming the perf bands the mismatch devalues) is required.
    A mismatch diagnostic without a note cannot be constructed."""

    level: str
    message: str
    note: str

    @classmethod
    def fail(cls, message: str) -> "Diag":
        return cls(FAILED, message, "")

    @classmethod
    def mismatch(cls, message: str, note: str) -> "Diag":
        return cls(INCOMPATIBLE, message, note)


SKIP_OPS_P99_NOTE = "skipping ops/s and p99 (environment class mismatch)"
SKIP_RSS_NOTE = "skipping rss ratio (environment class mismatch)"


def p99_breach_message(v: UsecaseVerdict, th: Thresholds) -> str:
    return (
        f"usecase {v.uid} p99 {v.cur_p99} > max({th.max_p99} * baseline {v.base_p99}, "
        f"baseline + {th.p99_jitter_ns}ns) (ops={v.ops or 0})"
    )


def memory_shape_checks(base: dict, cur: dict, th: Thresholds, diags: list[Diag]) -> None:
    """Memory-shape correctness walk (always runs, matched or not): the
    shape set and every shared shape's exact logical bytes (ADR-0289). The
    gate runs it before the kind's perf band, so every logical-bytes
    diagnostic keeps its place ahead of every ratio diagnostic."""
    b_shapes, c_shapes = shape_map(base), shape_map(cur)
    if set(b_shapes) != set(c_shapes):
        diags.append(
            Diag.fail(
                f"shape set {sorted(map(str, c_shapes))} "
                f"!= baseline {sorted(map(str, b_shapes))}"
            )
        )
    for key, b in b_shapes.items():
        c = c_shapes.get(key)
        if c is None:
            continue
        if shape_logical_breach(c.get("logical_bytes"), b.get("logical_bytes")):
            diags.append(
                Diag.fail(
                    f"{shape_label(key)} logical_bytes {c.get('logical_bytes')} "
                    f"!= baseline {b.get('logical_bytes')}"
                )
            )


def memory_shape_perf(base: dict, cur: dict, th: Thresholds, diags: list[Diag]) -> None:
    """Memory-shape perf band: the RSS-ratio walk (ADR-0289). The gate
    never runs it on an environment-class mismatch — a mismatch devalues
    the kind's perf band, exactly as for a generic pair."""
    b_shapes, c_shapes = shape_map(base), shape_map(cur)
    for key, b in b_shapes.items():
        c = c_shapes.get(key)
        if c is None:
            continue
        if rss_breach(c.get("ratio"), b.get("ratio"), th):
            diags.append(
                Diag.fail(
                    f"{shape_label(key)} rss ratio {c.get('ratio'):.4f} "
                    f"> {th.max_rss_ratio} * baseline {b.get('ratio'):.4f}"
                )
            )


def generic_correctness(base: dict, cur: dict, th: Thresholds, diags: list[Diag]) -> None:
    """Generic-pair correctness: the logical_used_bytes band, checked even
    on an environment-class mismatch — ADR-0071 keeps it correctness-only,
    so a mismatch never devalues it."""
    b_mem = (base.get("memory") or {}).get("logical_used_bytes") or 0
    c_mem = (cur.get("memory") or {}).get("logical_used_bytes") or 0
    if logical_breach(c_mem, b_mem, th):
        diags.append(
            Diag.fail(f"logical_used_bytes {c_mem} > {th.max_mem} * baseline {b_mem}")
        )


def generic_perf_checks(base: dict, cur: dict, th: Thresholds, diags: list[Diag]) -> None:
    """Throughput and per-use-case p99 checks (environments already matched)."""
    b_tp = (base.get("throughput") or {}).get("ops_per_sec") or 0.0
    c_tp = (cur.get("throughput") or {}).get("ops_per_sec") or 0.0
    if ops_breach(c_tp, b_tp, th):
        diags.append(
            Diag.fail(f"ops_per_sec {c_tp:.2f} < {th.min_tp} * baseline {b_tp:.2f}")
        )
    for v in usecase_verdicts(base, cur, th):
        if v.status == MISSING:
            diags.append(Diag.fail(f"missing usecase {v.uid}"))
        elif v.status == BREACH:
            diags.append(Diag.fail(p99_breach_message(v, th)))


class PairPolicy(NamedTuple):
    """One report kind's comparison policy — the single per-kind
    declaration the gate and the renderer both project from, keyed by the
    resolve_report_kind vocabulary (generic | memory-shape | checkpoint):
    the correctness walk that always runs, the perf band an
    environment-class mismatch devalues, the gate's skip line naming that
    band, and the renderer's devalued-rows phrase for the mismatch
    explainer. `perf` is never None — every kind has a perf band, even
    when (checkpoint) the walk is vacuous."""

    correctness: Callable[[dict, dict, Thresholds, list[Diag]], None]
    perf: Callable[[dict, dict, Thresholds, list[Diag]], None]
    skip_note: str
    devalued: str


PAIR_POLICIES: dict[str, PairPolicy] = {
    "generic": PairPolicy(
        correctness=generic_correctness,
        perf=generic_perf_checks,
        skip_note=SKIP_OPS_P99_NOTE,
        devalued="Wall-clock metrics (ops/s, p99) are skipped for this pair",
    ),
    "memory-shape": PairPolicy(
        correctness=memory_shape_checks,
        perf=memory_shape_perf,
        skip_note=SKIP_RSS_NOTE,
        devalued="RSS-ratio rows are skipped for this pair",
    ),
    # Checkpoint pairs are render-only in the pipeline (compare-baselines.sh
    # never gates them); gated directly, a checkpoint-shaped report — no
    # usecases, no throughput, no memory — satisfies the generic walk
    # vacuously, so the checkpoint gate protocol is the generic one.
    "checkpoint": PairPolicy(
        correctness=generic_correctness,
        perf=generic_perf_checks,
        skip_note=SKIP_OPS_P99_NOTE,
        devalued="Size rows are informational only",
    ),
}


def _is_checkpoint(report: dict) -> bool:
    """Checkpoint reports carry top-level size fields and no workload
    metrics. Fallback only: `--report-kind checkpoint` is the explicit
    dispatch the caller can make instead."""
    return (
        not report.get("usecases")
        and not report.get("throughput")
        and not report.get("memory")
        and any(k in report for k in ("logical_used_bytes", "rss_bytes", "data_file_bytes"))
    )


def resolve_report_kind(base: dict, cur: dict, explicit: str | None) -> str:
    """generic | memory-shape | checkpoint: the explicit flag wins, else the
    reports are sniffed (workload.kind, then the checkpoint shape). The one
    kind vocabulary: the renderer dispatches its table on it and the gate
    selects its policy record — the gate always sniffs (`explicit` is
    None there; `--report-kind` is a render-only flag)."""
    if explicit:
        return explicit
    if workload_kind(base) == "memory-shape" and workload_kind(cur) == "memory-shape":
        return "memory-shape"
    if _is_checkpoint(base) and _is_checkpoint(cur):
        return "checkpoint"
    return "generic"
