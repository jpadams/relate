#!/usr/bin/env python3
"""ci-timings-report: per-lane wall-time table for one CI run.

The manual "refresh the dashboard" command behind docs/testing/runtime.md
(LABS-310 round 4). Not a CI job; run it by hand when the runtime tables go
stale.

Usage:
    scripts/ci-timings-report.py [RUN_ID] [--no-logs]

RUN_ID defaults to the latest completed ci.yml run on the current branch.
Emits a markdown table on stdout: lane, conclusion, wall time, and the
job-end disk headroom the lane's `scripts/report-disk.sh` telemetry step
printed (`--no-logs` skips that log download).

Requires: gh (authenticated), python3.
"""

import argparse
import json
import subprocess
import sys
from datetime import datetime
from typing import Iterable

DISK_MARKER = "disk-headroom:"


def parse_disk_headroom(lines: Iterable[str]) -> dict[str, str]:
    """Extract per-lane disk headroom from `gh run view --log` output.

    Log lines are prefixed with the job name (`<job>\\t<step>\\t<line>`), so
    a flat scan groups per lane. Only lines whose text column carries the
    `disk-headroom:` marker that scripts/report-disk.sh prints count.
    """
    disk: dict[str, str] = {}
    for line in lines:
        parts = line.split("\t", 2)
        if len(parts) == 3 and DISK_MARKER in parts[2]:
            mb = parts[2].split(DISK_MARKER, 1)[1].strip().split(" MB")[0]
            disk[parts[0].strip()] = mb
    return disk


def render_report(run_id: str, data: dict, disk: dict[str, str]) -> str:
    """Render the paste-ready markdown table from the parsed
    `gh run view --json jobs` payload (fetch_run parses at the boundary)."""
    title = data["displayTitle"]
    created = data["createdAt"]
    out = [f"<!-- ci-timings-report: run {run_id} ({title}, {created}) -->"]
    out.append("| Lane | Conclusion | Wall | Disk free at job end (MB) |")
    out.append("| --- | --- | --- | --- |")
    rows = []
    for job in data["jobs"]:
        if not job.get("startedAt") or not job.get("completedAt"):
            continue
        t0 = datetime.fromisoformat(job["startedAt"].replace("Z", "+00:00"))
        t1 = datetime.fromisoformat(job["completedAt"].replace("Z", "+00:00"))
        rows.append((t1 - t0, job))
    for delta, job in sorted(rows, key=lambda r: r[0], reverse=True):
        secs = int(delta.total_seconds())
        wall = f"{secs // 60}m{secs % 60:02d}s"
        free = disk.get(job["name"], "—")
        out.append(f"| {job['name']} | {job['conclusion']} | {wall} | {free} |")
    return "\n".join(out)


def _gh(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    """The one gh subprocess policy: run gh, and exit loud on failure.

    `check=False` tolerates a failure (auth, network, bad run id) and hands
    the CompletedProcess back so the caller decides how to degrade.
    """
    proc = subprocess.run(["gh", *args], capture_output=True, text=True)
    if check and proc.returncode != 0:
        sys.exit(f"gh {' '.join(args[:2])} failed: {proc.stderr.strip()}")
    return proc


def latest_run_id(branch: str) -> str:
    out = _gh(
        "run", "list", "--workflow", "ci.yml", "--branch", branch,
        "--status", "completed", "--limit", "1", "--json", "databaseId",
    ).stdout
    runs = json.loads(out)
    if not runs or not runs[0].get("databaseId"):
        sys.exit("no completed ci.yml run found; pass a RUN_ID")
    return str(runs[0]["databaseId"])


def fetch_run(run_id: str) -> dict:
    return json.loads(
        _gh("run", "view", run_id, "--json", "jobs,displayTitle,createdAt").stdout
    )


def fetch_log(run_id: str) -> str:
    # The full log of a big run is tens of MB; tolerate gh hiccups on the
    # telemetry download — loudly, so a failed download is distinguishable
    # from a telemetry-less run. The table renders with '—' in the disk
    # column either way.
    proc = _gh("run", "view", run_id, "--log", check=False)
    if proc.returncode != 0:
        print(
            f"warning: gh run view --log failed: {proc.stderr.strip()}",
            file=sys.stderr,
        )
        return ""
    return proc.stdout


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="ci-timings-report",
        description="Per-lane wall-time + disk-headroom markdown table for one ci.yml run.",
    )
    parser.add_argument(
        "run_id",
        nargs="?",
        help="CI run id (default: latest completed ci.yml run on the current branch)",
    )
    parser.add_argument(
        "--no-logs",
        action="store_true",
        help="skip the run-log download (disk column renders '—')",
    )
    args = parser.parse_args()

    run_id = args.run_id
    if run_id is None:
        branch = subprocess.run(
            ["git", "branch", "--show-current"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        run_id = latest_run_id(branch)

    if args.no_logs:
        disk: dict[str, str] = {}
    else:
        disk = parse_disk_headroom(fetch_log(run_id).splitlines())
        if not disk:
            print("no disk-headroom telemetry found in run log", file=sys.stderr)

    print(render_report(run_id, fetch_run(run_id), disk))


if __name__ == "__main__":
    main()
