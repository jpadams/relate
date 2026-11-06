#!/usr/bin/env python3
"""Strip per-op arrays from a Relate loadtest JSON report for git-sized baselines."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def summarize(report: dict) -> dict:
    out = {k: v for k, v in report.items() if k != "ops"}
    out["ops_omitted"] = True
    out["ops_count"] = len(report.get("ops") or [])
    return out


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("src", type=Path)
    p.add_argument("dst", type=Path)
    args = p.parse_args()
    report = json.loads(args.src.read_text())
    args.dst.parent.mkdir(parents=True, exist_ok=True)
    args.dst.write_text(json.dumps(summarize(report), indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
