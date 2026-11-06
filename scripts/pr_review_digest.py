#!/usr/bin/env python3
"""pr_review_digest: newest-first digest of a PR's review comments.

The manual step behind AGENTS.md's "read review comments incrementally,
newest first" rule. A PR accumulates bot comments fast (per-round stellar,
rust, security, code reviews plus status bots), and only the latest round
is current — earlier rounds' findings may already be fixed or superseded.
This script does the fetch-and-order mechanically so the reader starts at
the newest review-skill comment per reviewer instead of dumping the full
comment list into context.

Usage:
    scripts/pr_review_digest.py <PR> [--repo OWNER/NAME] [--limit N]
                                [--since-id ID] [--full]

Prints one block per comment, NEWEST first: id, author, created_at, a
`superseded` marker when the same author has a newer comment (skip those
unless the current round references them), then the body (first 40 lines
by default; `--full` for all). `--since-id ID` prints only comments with
id > ID — pass the previous run's `last-comment-id` line to read only
what landed since. `--limit N` caps the number of blocks.

The final line is `last-comment-id: <newest id>` — the next invocation's
`--since-id`. Requires: gh (authenticated), python3.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from typing import Any, NamedTuple

BODY_HEAD_LINES = 40


class DigestError(Exception):
    """Typed failure — never a silent fallback over malformed comment JSON."""


class Comment(NamedTuple):
    """One normalized comment — the only shape digest phases operate on."""

    id: int
    login: str
    created_at: str
    body: str


def _run_gh(args: list[str]) -> str:
    """Run one `gh` invocation, raising DigestError on any failure."""
    proc = subprocess.run(args, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise DigestError(
            f"gh failed ({proc.returncode}): {proc.stderr.strip()}"
        )
    return proc.stdout


def resolve_repo() -> str:
    """The checkout's OWNER/NAME — the I/O-layer counterpart of fetch."""
    out = _run_gh(
        ["gh", "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"]
    )
    repo = out.strip()
    if not repo:
        raise DigestError("gh repo view returned an empty repository name")
    return repo


def to_comments(payload: Any) -> list[Comment]:
    """Pure payload→records conversion: envelope check + normalize map."""
    if not isinstance(payload, list):
        raise DigestError("payload is not a list of issue comments")
    return [normalize(comment) for comment in payload]


def fetch_comments(repo: str, pr: int) -> list[Comment]:
    """Fetch and normalize every issue comment on the PR via `gh api`.

    The typed boundary sits at the I/O edge: the gh payload flows through
    `to_comments` here, so everything downstream operates on `Comment`
    records only.
    """
    endpoint = f"repos/{repo}/issues/{pr}/comments?per_page=100"
    out = _run_gh(["gh", "api", "--paginate", endpoint])
    try:
        payload = json.loads(out)
    except json.JSONDecodeError as exc:
        raise DigestError(f"gh api returned non-JSON output: {exc}") from exc
    return to_comments(payload)


def _field(container: dict[str, Any], key: str, kind: type) -> Any:
    """Read one required field with its expected type — the one policy.

    `type(...) is kind` (not isinstance) so a bool never passes as an int.
    """
    if key not in container:
        raise DigestError(f"entry without '{key}': {json.dumps(container)[:200]}")
    value = container[key]
    if type(value) is not kind:
        raise DigestError(
            f"field '{key}' is not {kind.__name__}: {value!r}"
        )
    return value


def normalize(comment: Any) -> Comment:
    """Validate one raw comment dict into a typed record, once.

    Every malformed shape — non-dict entry, missing key, wrong type,
    explicit-null body — exits through this single DigestError path.
    """
    if not isinstance(comment, dict):
        raise DigestError(f"comment entry is not an object: {comment!r}")
    user = _field(comment, "user", dict)
    return Comment(
        id=_field(comment, "id", int),
        login=_field(user, "login", str),
        created_at=_field(comment, "created_at", str),
        body=_field(comment, "body", str),
    )


def digest_lines(
    records: list[Comment],
    since_id: int = 0,
    limit: int | None = None,
    full: bool = False,
) -> list[str]:
    """Render the digest: newest-first blocks, superseded markers, last id.

    Takes normalized `Comment` records (validated at the I/O edge by
    `fetch_comments`); rendering never re-reads raw JSON. Ordering is
    (created_at, id) descending — the id tie-break keeps same-second
    bursts deterministic. `superseded` marks every comment that is not
    its author's newest (the author's first appearance under the display
    ordering), so a round's current verdict is the first block per
    reviewer.
    """
    ordered = sorted(records, key=lambda r: (r.created_at, r.id), reverse=True)
    newest_by_author: dict[str, int] = {}
    for record in ordered:
        newest_by_author.setdefault(record.login, record.id)

    fresh = [r for r in ordered if r.id > since_id]
    shown = fresh if limit is None else fresh[:limit]

    lines: list[str] = []
    for record in shown:
        marker = "" if newest_by_author[record.login] == record.id else "  superseded"
        lines.append(
            f"--- comment {record.id}  {record.login}  {record.created_at}{marker}"
        )
        body_lines = record.body.splitlines()
        head = body_lines if full else body_lines[:BODY_HEAD_LINES]
        lines.extend(head)
        if not full and len(body_lines) > BODY_HEAD_LINES:
            lines.append(f"… ({len(body_lines) - BODY_HEAD_LINES} more lines, --full)")
        lines.append("")

    lines.append(f"last-comment-id: {max((r.id for r in records), default=0)}")
    return lines


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="pr_review_digest",
        description="Newest-first digest of a PR's review comments.",
    )
    parser.add_argument("pr", type=int, help="PR number")
    parser.add_argument(
        "--repo",
        default=None,
        help="OWNER/NAME (default: the gh checkout's repository)",
    )
    parser.add_argument(
        "--limit", type=int, default=None, help="print at most N comment blocks"
    )
    parser.add_argument(
        "--since-id",
        type=int,
        default=0,
        help="print only comments with id > ID (previous last-comment-id)",
    )
    parser.add_argument("--full", action="store_true", help="print whole bodies")
    args = parser.parse_args(argv)

    try:
        repo = args.repo if args.repo is not None else resolve_repo()
        comments = fetch_comments(repo, args.pr)
        for line in digest_lines(comments, args.since_id, args.limit, args.full):
            print(line)
    except DigestError as exc:
        print(f"pr-review-digest: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
