#!/usr/bin/env python3
"""Mine query patterns out of an openCypher TCK corpus (references/sources.md).

Walks every .feature under the given directory, extracts each scenario's
executed query (the `When executing query:` block), maps the feature file to a
coverage area, and emits workload-format JSONL. Expectations and graphs are not
carried — execution against the real dataset is the truth (the patterns get
rewritten onto the dataset schema per references/generation-recipes.md).

    python3 mine_tck.py /path/to/harness/features/tck --out tck_patterns.jsonl
"""

import argparse
import json
import re
import sys
from pathlib import Path

AREA_BY_DIR = {
    "clauses/match-where": "where",
    "clauses/return-orderby": "order-by",
    "clauses/return-skip-limit": "skip-limit",
}
# Raw openCypher corpus dirs -> coverage-map areas (the spec-harness
# checkout's `features/tck/` layout, same as any openCypher TCK checkout).
AREA_BY_RAW_DIR = {
    "expressions/quantifier": "predicate-functions",
    "expressions/aggregating": "aggregating-functions",
    "expressions/string": "string-functions",
    "expressions/list": "list-expressions",
    "expressions/map": "map-expressions",
    "expressions/mathematical": "mathematical-operators",
    "expressions/temporal": "temporal-functions",
    "expressions/precedence": "boolean-operators",
    "expressions/literals": "comparison-operators",
    "expressions/null": "comparison-operators",
    "expressions/typeConversion": "scalar-functions",
    "expressions/countingSubgraphMatches": "aggregating-functions",
    "expressions/existentialSubqueries": "subqueries-collect-count-exists",
    "expressions/pattern": "path-pattern-expressions",
    "graphs": "variable-length-paths",
    "useCases": "match",
}

AREA_BY_STEM = [
    (r"^MatchWhere", "where"),
    (r"^Match", "match"),
    (r"^OptionalMatch", "optional-match"),
    (r"^ReturnOrderby|^ReturnOrderBy", "order-by"),
    (r"^ReturnSkipLimit", "skip-limit"),
    (r"^Return", "return"),
    (r"^With", "with"),
    (r"^Unwind", "unwind"),
    (r"^Create", "create"),
    (r"^Merge", "merge"),
    (r"^Delete", "delete"),
    (r"^Remove", "remove"),
    (r"^Set", "set"),
    (r"^Call", "subqueries-call"),
    (r"^Aggregation|^Aggregate", "aggregating-functions"),
    (r"^Expression", "conditional-expressions"),
    (r"^List", "list-expressions"),
    (r"^Map", "map-expressions"),
    (r"^String", "string-functions"),
    (r"^Comparison", "comparison-operators"),
    (r"^Boolean", "boolean-operators"),
    (r"^Path|^ShortestPath|^VarLength", "variable-length-paths"),
    (r"^Temporal|^Duration|^Timestamp", "temporal-functions"),
    (r"^Point|^Spatial", "spatial-functions"),
    (r"^Type", "type-predicate-expressions"),
    (r"^Label", "label-expressions"),
]

WRITE_AREAS = {"create", "merge", "delete", "remove", "set"}

QUERY_BLOCK = re.compile(
    r"^\s*When executing query:\s*\n\s*\"\"\"\s*\n(.*?)^\s*\"\"\"\s*$",
    re.M | re.S,
)
PARAMS_BLOCK = re.compile(r"^\s*And parameters are:\s*\n((?:\s*\|.*\|\s*\n)+)", re.M)
SCENARIO = re.compile(r"^\s*(?:Scenario|Scenario Outline)(?:\s*Outline)?:\s*(.+)$", re.M)
# TCK Then-lines carry the expectation: "Then a SyntaxError should be raised
# at compile time: VariableAlreadyBound" vs "Then the result should be ..."
EXPECT_ERROR = re.compile(
    r"Then an? (\w+) should be raised at (compile|runtime)(?: time)?(?:: (\w+))?"
)


# Two ordered rule classes, first hit wins within each. Dir rules match the
# full posix path (so a directory anywhere in the tree claims the file);
# stem rules match the file's stem. Dir rules run first, so a stem match
# cannot steal a file from its named area.
DIR_RULES = [(rf"/{re.escape(d)}/", area) for d, area in {**AREA_BY_RAW_DIR, **AREA_BY_DIR}.items()]
STEM_RULES = list(AREA_BY_STEM)


def area_of(path: Path) -> str:
    posix = path.as_posix()
    for pattern, area in DIR_RULES:
        if re.search(pattern, posix):
            return area
    for pattern, area in STEM_RULES:
        if re.search(pattern, path.stem):
            return area
    return path.parent.name


SCENARIO_BLOCK = re.compile(
    r"(?ms)^\s*Scenario(?P<outline> Outline)?: (.+?)$\n(.*?)(?=^\s*Scenario(?: Outline)?: |\Z)"
)


def scenarios(text: str):
    """Yield one dict per scenario block: name, query, params, expectation,
    and skip_reason when the block cannot run as-is.

    Block-based, so a scenario's params and its Then-expectation stay attached
    to its own query. Scenario Outlines carry `<placeholder>` columns the
    validator cannot run — the parser saw the Outline keyword, so those are
    marked here, not sniffed out of the query text. A params table that fails
    to parse is also skip-with-reason — never silently unparameterized.
    """
    for m in SCENARIO_BLOCK.finditer(text):
        name = m.group(2).strip()
        body = m.group(3)
        q = QUERY_BLOCK.search(body)
        if not q:
            continue
        out = {"name": name, "query": q.group(1).strip("\n"), "params": {}}
        reasons = []
        pm = PARAMS_BLOCK.search(body)
        if pm:
            lines = pm.group(1).strip().splitlines()
            keys = [c.strip() for c in lines[0].strip("|").split("|")]
            vals = [c.strip() for c in lines[-1].strip("|").split("|")]
            try:
                if len(keys) != len(vals):
                    raise ValueError(f"params arity {len(keys)} != {len(vals)}")
                out["params"] = {k: json.loads(v) for k, v in zip(keys, vals)}
            except (json.JSONDecodeError, ValueError) as exc:
                reasons.append(f"params table unparsable ({exc})")
        if m.group("outline"):
            reasons.append("scenario outline placeholders not substituted")
        if reasons:
            out["skip_reason"] = "; ".join(reasons)
        em = EXPECT_ERROR.search(body)
        out["expectation"] = (
            {"expect": "ok"}
            if not em
            else {
                "expect": "error",
                "expect_kind": em.group(1),
                "expect_phase": em.group(2),
                "expect_detail": em.group(3),
            }
        )
        yield out


def tier_of(query: str) -> str:
    """Structural tiers (coverage-map.md): a *fixed* hop count (`*2`, `*1..1`)
    is never complex; only genuinely variable-length paths are."""
    upper = query.upper()
    if re.search(r"\bSHORTEST\b|\bLET\b|\bFILTER\b|\bWHEN\b|\bFOR\b", upper):
        return "complex"
    # only quantifier-shaped stars reach here: a bare `*` (count(*), `.*`)
    # does not match, so aggregation queries are not misread as path-quantified
    for m in re.finditer(r"\*\s*(?:(\d+)\s*(?:\.\.\s*(\d+)?)?|\.\.\s*(?:\d+)?)", query):
        lo, hi, g = m.group(1), m.group(2), m.group(0)
        # fixed hop count: *n, or a degenerate range *n..n — never complex by itself
        fixed = lo is not None and (".." not in g or hi == lo)
        if not fixed:
            return "complex"
    if re.search(r"\bWITH\b|\bUNION\b|\bCALL\b|\bOPTIONAL\b|\bCASE\b|\bFOREACH\b|EXISTS|COUNT\s*\{|\[", upper):
        return "mid"
    return "simple"


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("corpus", type=Path, help="TCK directory (clauses/, expressions/, ...)")
    ap.add_argument("--out", type=Path, default=Path("tck_patterns.jsonl"))
    args = ap.parse_args()

    entries = []
    for feature in sorted(args.corpus.rglob("*.feature")):
        text = feature.read_text(encoding="utf-8", errors="replace")
        area = area_of(feature)
        for sc in scenarios(text):
            query = sc["query"]
            if not query.strip():
                continue
            entry = {
                "id": f"tck.{feature.stem}.{len(entries):04d}",
                "area": area,
                "tier": tier_of(query),
                "kind": "write" if area in WRITE_AREAS else "read",
                "query": query,
                "params": sc["params"],
                "source": f"tck:{feature.stem}::{sc['name']}",
            }
            entry.update(sc["expectation"])
            if sc.get("skip_reason"):
                entry["skip_reason"] = sc["skip_reason"]
            entries.append(entry)

    args.out.write_text("".join(json.dumps(e) + "\n" for e in entries))
    by_area = {}
    for e in entries:
        by_area[e["area"]] = by_area.get(e["area"], 0) + 1
    print(f"{len(entries)} patterns from {args.corpus} -> {args.out}")
    for area, n in sorted(by_area.items()):
        print(f"  {area}: {n}")


if __name__ == "__main__":
    main()
