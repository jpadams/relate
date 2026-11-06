#!/usr/bin/env python3
"""Freeze a worst-of-N envelope baseline from repeated loadtest reports.

A single run is not reproducible in the 2 vCPU baseline container (ADR-0070):
tail latency is bimodal for short use cases. The envelope keeps, per gated
metric, the value that makes the gate hardest to pass by accident and easiest
for a healthy run to meet: lowest throughput, highest per-use-case p99, highest
logical bytes. `scripts/compare-loadtest.py` then applies its unchanged
ADR-0046 bands on top of that envelope.

Per-op arrays are stripped, as in `scripts/summarize-loadtest.py`.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def load(path: Path) -> dict:
    return json.loads(path.read_text())


def memory_shape(rep: dict) -> bool:
    return (rep.get("workload") or {}).get("kind") == "memory-shape"


def shape_key(shape: dict) -> tuple:
    return (shape.get("shape"), shape.get("index"))


def check_compatible(reports: list[tuple[Path, dict]]) -> list[str]:
    problems: list[str] = []
    first_path, first = reports[0]
    schema = first.get("schema_version")
    kind = (first.get("workload") or {}).get("kind")
    if first.get("environment") is None:
        problems.append(f"{first_path}: missing environment (ADR-0071)")
    if memory_shape(first):
        keys = [shape_key(s) for s in first.get("shapes") or []]
        logical = {shape_key(s): s.get("logical_bytes") for s in first.get("shapes") or []}
        for path, rep in reports:
            if rep.get("schema_version") != schema:
                problems.append(f"{path}: schema_version {rep.get('schema_version')} != {schema} ({first_path})")
            if (rep.get("workload") or {}).get("kind") != kind:
                problems.append(f"{path}: workload.kind differs from {first_path}")
            if first.get("environment") != rep.get("environment"):
                problems.append(f"{path}: environment differs from {first_path}")
            if [shape_key(s) for s in rep.get("shapes") or []] != keys:
                problems.append(f"{path}: shape set differs from {first_path}")
            for shape in rep.get("shapes") or []:
                key = shape_key(shape)
                if shape.get("logical_bytes") != logical.get(key):
                    problems.append(
                        f"{path}: logical_bytes for {key[0]}/{key[1]} "
                        f"{shape.get('logical_bytes')} != {logical.get(key)}"
                    )
        return problems
    ids = {u["id"] for u in first.get("usecases") or []}
    for path, rep in reports:
        if rep.get("schema_version") != schema:
            problems.append(f"{path}: schema_version {rep.get('schema_version')} != {schema} ({first_path})")
        if (rep.get("workload") or {}).get("kind") != kind:
            problems.append(f"{path}: workload.kind differs from {first_path}")
        if first.get("environment") != rep.get("environment"):
            problems.append(f"{path}: environment differs from {first_path}")
        if {u["id"] for u in rep.get("usecases") or []} != ids:
            problems.append(f"{path}: use-case set differs from {first_path}")
        for u in rep.get("usecases") or []:
            if u.get("errors"):
                problems.append(f"{path}: usecase {u['id']} has errors={u['errors']}")
    return problems


def envelope(reports: list[tuple[Path, dict]]) -> dict:
    if memory_shape(reports[-1][1]):
        return envelope_memory(reports)
    # Structure comes from the last run; gated metrics are widened below.
    out = {k: v for k, v in reports[-1][1].items() if k != "ops"}
    out["ops_omitted"] = True
    out["ops_count"] = len(reports[-1][1].get("ops") or [])

    throughputs = [
        (r.get("throughput") or {}).get("ops_per_sec")
        for _, r in reports
        if (r.get("throughput") or {}).get("ops_per_sec")
    ]
    if throughputs and out.get("throughput"):
        out["throughput"] = dict(out["throughput"])
        out["throughput"]["ops_per_sec"] = min(throughputs)

    logical = [
        (r.get("memory") or {}).get("logical_used_bytes")
        for _, r in reports
        if (r.get("memory") or {}).get("logical_used_bytes")
    ]
    if logical and out.get("memory"):
        out["memory"] = dict(out["memory"])
        out["memory"]["logical_used_bytes"] = max(logical)

    worst: dict[str, dict[str, int]] = {}
    for _, rep in reports:
        for u in rep.get("usecases") or []:
            lat = u.get("latency_ns")
            if not lat:
                continue
            cur = worst.setdefault(u["id"], {})
            for stat, value in lat.items():
                if isinstance(value, (int, float)):
                    cur[stat] = max(value, cur.get(stat, value))

    if out.get("usecases"):
        merged = []
        for u in out["usecases"]:
            u = dict(u)
            if u.get("latency_ns") and worst.get(u["id"]):
                u["latency_ns"] = {**u["latency_ns"], **worst[u["id"]]}
            merged.append(u)
        out["usecases"] = merged

    out["baseline_envelope"] = {
        "policy": "worst-of-n",
        "runs": len(reports),
        "sources": [p.name for p, _ in reports],
        "throughput_ops_per_sec": "min",
        "usecase_latency_ns": "max",
        "logical_used_bytes": "max",
    }
    env = reports[0][1].get("environment")
    if env is not None:
        out["environment"] = env
    return out


def envelope_memory(reports: list[tuple[Path, dict]]) -> dict:
    """Keep the highest RSS ratio per shape. Logical bytes are exact."""
    out = {k: v for k, v in reports[-1][1].items() if k != "ops"}
    best: dict[tuple, dict] = {}
    for _, rep in reports:
        for shape in rep.get("shapes") or []:
            key = shape_key(shape)
            prev = best.get(key)
            if prev is None or (shape.get("ratio") or 0) > (prev.get("ratio") or 0):
                best[key] = shape
    out["shapes"] = [best[shape_key(shape)] for shape in out.get("shapes") or []]
    out["baseline_envelope"] = {
        "policy": "worst-of-n",
        "runs": len(reports),
        "sources": [p.name for p, _ in reports],
        "rss_ratio": "max",
        "logical_bytes": "exact",
    }
    env = reports[0][1].get("environment")
    if env is not None:
        out["environment"] = env
    return out


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("reports", type=Path, nargs="+")
    args = p.parse_args()

    reports = [(path, load(path)) for path in args.reports]
    problems = check_compatible(reports)
    if problems:
        for problem in problems:
            print(f"freeze-baseline-envelope: {problem}", file=sys.stderr)
        return 1

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(envelope(reports), indent=2) + "\n")
    print(f"freeze-baseline-envelope: {args.out} (worst of {len(reports)} runs)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
