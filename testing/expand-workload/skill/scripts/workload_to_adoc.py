#!/usr/bin/env python3
"""Render a validated workload as an executable AsciiDoc guide — a feature history.

One section per round, entries in growth order (`extends` chains walk after their
parents), each with its provenance note (base / growth operator / capability /
inspiration) and its validated outcome. Runs with the skill's validator:

    python3 run_workload.py stackoverflow-workload.adoc --engine relate --store db.rl

Metadata serializes as block attributes:
`[source,cypher,id="r2.paging.003",round="2",extends="r0.top-tags.001",
expect="error:Detail",expect_ms="500",slow,skip="reason"]`.

    python3 workload_to_adoc.py workload.jsonl report.json --themes themes.json --out workload.adoc

`themes.json` (optional): {"1": {"title": "paging arrives", "prose": "…"}} — the
round's story line; rounds without an entry get a bare title.
"""

import argparse
import collections
import json
from pathlib import Path

MUTATION_NOTES = {
    "dir-flip": "reversed a relationship direction",
    "opt-wrap": "turned a MATCH into OPTIONAL MATCH with a null-safe projection",
    "distinct": "toggled DISTINCT",
    "agg-swap": "swapped the aggregate (count/collect, avg/sum, min/max)",
    "limit-page": "turned LIMIT into SKIP/LIMIT paging",
    "param-lift": "lifted a literal into a parameter",
    "with-split": "split the projection across a WITH boundary",
    "order-expr": "ordered by a non-returned expression",
    "label-add": "added a label condition",
    "label-drop": "dropped a label condition",
}


def note_for(entry, results):
    src = entry.get("source", "")
    parent = entry.get("extends")
    parts = []
    if src.startswith("rewrite:"):
        src = src[len("rewrite:"):]
    if src.startswith("seed:"):
        parts.append(f"Base query from {src[5:]}.")
    elif src.startswith("mutation:"):
        _, op, base = src.split(":", 2)
        parts.append(f"Mutation `{op}` of {base}: {MUTATION_NOTES.get(op, op)}.")
    elif src.startswith("growth:"):
        # the source names only the capability; the parent is `extends`
        capability = src.split(":", 1)[1]
        parts.append(
            f"Grows `{parent}` with the round's capability ({capability})."
            if parent
            else f"Introduces the round's capability ({capability})."
        )
    elif src.startswith("tck:"):
        parts.append(
            f"Shape inspired by the openCypher TCK `{src[4:]}`, rewritten onto the dataset."
        )
    elif src.startswith("composed"):
        parts.append("Introduced for coverage of this area.")
    if parent and not src.startswith("growth:"):
        parts.append(f"Extends `{parent}`.")
    if entry.get("expect") == "error":
        parts.append(f"Expected to fail with `{entry.get('expect_detail') or 'an error'}`.")
    r = results.get(entry.get("id"))
    if r:
        if r["status"] == "ok":
            parts.append(f"{r['rows']} rows, {r['median_ms']} ms.")
        elif r["status"] == "expected-error":
            parts.append("Fails as expected — matched expectation.")
        elif r["status"] in ("unexpected-ok", "wrong-error"):
            parts.append(f"**Mismatch: {r['status'].replace('-', ' ')}** — see the defect list.")
        elif r["status"] == "engine-error":
            parts.append(f"**Engine error**: `{r['error']}`")
        elif r["status"] == "timeout":
            parts.append("**Timed out** in validation.")
        elif r["status"] == "skipped":
            parts.append(f"Skipped: {r['error']}")
        if r.get("slow"):
            parts.append("Slow (over budget).")
    return " ".join(parts)


def attrs_for(entry):
    # kind is always emitted: the round sections are narrative headers, so a
    # write query that omitted it would reload as a read. Every other
    # attribute follows the same emit-only-when-present policy.
    attrs = (
        f'source,cypher,id="{entry["id"]}",round="{entry.get("round", 0)}",'
        f'kind="{entry.get("kind", "read")}"'
    )
    if entry.get("tier"):
        attrs += f',tier="{entry["tier"]}"'
    if entry.get("area"):
        attrs += f',area="{entry["area"]}"'
    if entry.get("params"):
        attrs += ",params='" + json.dumps(entry["params"]) + "'"
    if entry.get("extends"):
        attrs += f',extends="{entry["extends"]}"'
    if entry.get("expect") == "error":
        detail = entry.get("expect_detail") or ""
        attrs += f',expect="error:{detail}"'
    if entry.get("expect_ms"):
        attrs += f',expect_ms="{entry["expect_ms"]}"'
    if entry.get("expect_slow"):
        attrs += ",slow"
    if entry.get("skip_reason"):
        attrs += f',skip="{entry["skip_reason"]}"'
    return attrs


def ordered(entries):
    """Base introductions first, then growth entries after their parents."""
    by_id = {e["id"]: e for e in entries}
    seen = set()
    out = []

    def emit(e):
        if e["id"] in seen:
            return
        parent = by_id.get(e.get("extends"))
        if parent and parent["id"] not in seen and parent.get("round", 0) == e.get("round", 0):
            emit(parent)
        seen.add(e["id"])
        out.append(e)

    for e in sorted(entries, key=lambda e: (e.get("extends") is not None, e["id"])):
        emit(e)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("workload", type=Path)
    ap.add_argument("report", type=Path, help="run_workload.py report.json")
    ap.add_argument("--title", default="Expanded query workload")
    ap.add_argument("--intro", default="")
    ap.add_argument("--themes", type=Path, help="round -> {title, prose} JSON")
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    entries = [json.loads(l) for l in args.workload.read_text().splitlines() if l.strip()]
    report = json.loads(args.report.read_text())
    results = {q["id"]: q for q in report["queries"]}
    themes = json.loads(args.themes.read_text()) if args.themes else {}

    out = [f"= {args.title}", ""]
    if args.intro:
        out += [args.intro, ""]
    # counts are None when the report came from the CLI backend (leak check
    # disabled) — the guide still renders, with unknown counts said in the open
    counts = report["dataset_counts"] or {"nodes": "?", "rels": "?"}
    s = report["summary"]
    out += [
        f"Grown by the expand-workload skill and validated against a store of "
        f"{counts['nodes']} nodes / {counts['rels']} relationships on `{report['engine']}`.",
        "",
        "Validation at generation time: "
        + ", ".join(f"{k} {v}" for k, v in sorted(s.items()) if k != "total")
        + f" of {s['total']} entries.",
        "",
    ]

    by_round = collections.defaultdict(list)
    for e in entries:
        by_round[e.get("round", 0)].append(e)
    for rnd in sorted(by_round):
        theme = themes.get(str(rnd), {})
        title = theme.get("title", f"round {rnd}")
        out += [f"== Round {rnd} — {title}", ""]
        if theme.get("prose"):
            out += [theme["prose"], ""]
        for e in ordered(by_round[rnd]):
            note = note_for(e, results)
            out += [f"{note}\n\n[{attrs_for(e)}]\n----\n{e['query'].strip()}\n----\n", ""]
    args.out.write_text("\n".join(out).rstrip() + "\n")
    print(f"{len(entries)} entries in {len(by_round)} rounds -> {args.out}")


if __name__ == "__main__":
    main()
