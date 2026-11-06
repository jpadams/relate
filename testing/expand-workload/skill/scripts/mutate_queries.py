#!/usr/bin/env python3
"""Mechanical mutation of seed queries (references/generation-recipes.md).

Applies the mutation operators to every seed entry — one operator per variant —
so a handful of known-good queries becomes a broader set without invention.
String surgery only; the validator (run_workload.py) decides what survives.

    python3 mutate_queries.py seeds.jsonl --out mutations.jsonl
"""

import argparse
import json
import re
from pathlib import Path


def flip_direction(query):
    """dir-flip: reverse the first directed relationship."""
    flipped, n = re.subn(r"-\[([^\]]*)\]->", "<-[\\1]-", query, count=1)
    return (flipped, {}) if n else None


def opt_wrap(query):
    """opt-wrap: first plain MATCH becomes OPTIONAL MATCH; null-safe projection."""
    wrapped, n = re.subn(r"(?m)^(\s*)MATCH\s", r"\1OPTIONAL MATCH ", query, count=1)
    return (wrapped, {}) if n else None


def distinct(query):
    """distinct: add DISTINCT to the outermost RETURN (or remove an existing one)."""
    if re.search(r"\bRETURN\s+DISTINCT\b", query, re.I):
        return (re.sub(r"\bRETURN\s+DISTINCT\b", "RETURN", query, flags=re.I, count=1), {})
    return (re.sub(r"\bRETURN\s", "RETURN DISTINCT ", query, flags=re.I, count=1), {})


def agg_swap(query):
    """agg-swap: count -> collect, avg -> sum, min -> max (first hit)."""
    for a, b in ((r"\bcount\s*\(", "collect("), (r"\bavg\s*\(", "sum("), (r"\bmin\s*\(", "max(")):
        swapped, n = re.subn(a, b, query, flags=re.I, count=1)
        if n:
            return (swapped, {})
    return None


def limit_page(query):
    """limit-page: LIMIT n -> SKIP k LIMIT k (k = n // 2)."""
    m = re.search(r"\bLIMIT\s+(\d+)", query, re.I)
    if not m:
        return None
    n = int(m.group(1))
    k = max(1, n // 2)
    return (query[: m.start()] + f"SKIP {k} LIMIT {k}" + query[m.end():], {})


def param_lift(query):
    """param-lift: first single-quoted string literal becomes $p1."""
    m = re.search(r"'([^']*)'", query)
    if not m:
        return None
    return query[: m.start()] + "$p1" + query[m.end():], {"p1": m.group(1)}


def with_split(query):
    """with-split: insert WITH * before the last RETURN (projection boundary)."""
    m = list(re.finditer(r"\bRETURN\b", query, re.I))
    if len(m) < 1 or "WITH" in query.upper():
        return None
    last = m[-1]
    return (query[: last.start()] + "WITH * " + query[last.start():], {})


def order_expr(query):
    """order-expr: ORDER BY a non-returned expression on the first bound variable."""
    if re.search(r"\bORDER\s+BY\b", query, re.I):
        return None
    m = re.search(r"\bMATCH\s*\(\s*([a-zA-Z_]\w*)", query)
    if not m:
        return None
    return (query.rstrip() + f" ORDER BY size(keys({m.group(1)})) DESC", {})


OPERATORS = {
    "dir-flip": flip_direction,
    "opt-wrap": opt_wrap,
    "distinct": distinct,
    "agg-swap": agg_swap,
    "limit-page": limit_page,
    "param-lift": param_lift,
    "with-split": with_split,
    "order-expr": order_expr,
}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("seeds", type=Path, help="workload-format JSONL")
    ap.add_argument("--out", type=Path, default=Path("mutations.jsonl"))
    ap.add_argument("--operators", nargs="*", default=list(OPERATORS), choices=list(OPERATORS))
    args = ap.parse_args()

    out = []
    for line in args.seeds.read_text().splitlines():
        if not line.strip():
            continue
        seed = json.loads(line)
        for op in args.operators:
            # uniform contract: every operator returns (query, extra_params)
            # or None — no ad-hoc union dispatched by isinstance
            result = OPERATORS[op](seed["query"])
            if result is None:
                continue
            query, extra = result
            params = dict(seed.get("params") or {})
            params.update(extra)
            if query.strip() == seed["query"].strip():
                continue
            out.append(
                {
                    "id": f"{seed.get('id', 'seed')}.{op}",
                    "area": seed.get("area"),
                    "tier": seed.get("tier"),
                    "kind": seed.get("kind", "read"),
                    "query": query,
                    "params": params,
                    "source": f"mutation:{op}:{seed.get('source', seed.get('id', 'seed'))}",
                }
            )

    args.out.write_text("".join(json.dumps(e) + "\n" for e in out))
    print(f"{len(out)} mutations -> {args.out}")


if __name__ == "__main__":
    main()
