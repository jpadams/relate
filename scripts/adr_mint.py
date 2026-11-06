#!/usr/bin/env python3
"""Claim the next ADR number from the shared counter branch.

Parallel agents each computing "highest docs/adr prefix + 1" from their own
checkout collide on the same number. The shared ledger is a remote branch
(COUNTER_BRANCH in adr_lib), holding a single file NEXT with the next
unclaimed four-digit number. Claiming is a compare-and-swap push: git ref
updates are atomic on the server, so a non-fast-forward rejection means
someone else claimed first — the script refetches, recomputes, and retries
(up to 10 attempts).

On success, stdout carries two lines: the minted number, zero-padded to
four digits, then the 40-hex SHA of the claim commit just pushed to the
counter branch. The caller names docs/adr/NNNN-<slug>.md itself. Numbers
are never minted below 0300: in-flight branches may hold 0243-0299 claimed
under the old list-the-directory rule before those ADRs exist on main. The
bump commit is built with git plumbing only (hash-object / mktree /
commit-tree) and pushed as <commit>:refs/heads/<COUNTER_BRANCH> — the
script never checks out a branch and never touches the worktree or the
index of the repository it runs in. It anchors itself to the repository
root (git rev-parse --show-toplevel), so the docs/adr scan is independent
of the directory it is invoked from.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from adr_lib import (
    COUNTER_BRANCH,
    adr_entries,
    anchor_repo_root,
    checked_git,
    claim_subject,
    die,
    git,
    list_adr_paths,
)

PROGRAM = "adr_mint"
MAX_ATTEMPTS = 10
# Never mint below 0300: in-flight branches may already hold numbers in the
# 0243-0299 range grabbed under the old list-the-directory rule, and those
# ADRs do not exist on main yet.
FLOOR = 300


def rev_parse(rev: str) -> str:
    result = checked_git(
        PROGRAM, f"cannot resolve {rev}", "rev-parse", "--verify", rev
    )
    return result.stdout.strip()


def remote_tip(remote: str, ref: str) -> str | None:
    """The remote's tip for ref, or None when the remote lacks it."""
    result = checked_git(
        PROGRAM, f"cannot query remote {remote!r}", "ls-remote", remote, ref
    )
    line = result.stdout.strip()
    return line.split(None, 1)[0] if line else None


def fetch_main(remote: str) -> str:
    checked_git(
        PROGRAM,
        f"cannot fetch main from {remote!r}",
        "fetch",
        "--no-tags",
        remote,
        "main",
    )
    return rev_parse("FETCH_HEAD")


def fetch_counter(remote: str) -> str | None:
    """Fetch the counter branch; return its tip, or None when absent."""
    ref = f"refs/heads/{COUNTER_BRANCH}"
    if remote_tip(remote, ref) is None:
        return None
    checked_git(
        PROGRAM,
        f"cannot fetch {ref} from {remote!r}",
        "fetch",
        "--no-tags",
        remote,
        COUNTER_BRANCH,
    )
    return rev_parse("FETCH_HEAD")


def max_adr_prefix(main_tip: str) -> int:
    """Highest NNNN prefix of docs/adr/NNNN-*.md on main (0 when none)."""
    return max(
        (int(number) for number, _ in adr_entries(list_adr_paths(PROGRAM, main_tip))),
        default=0,
    )


def read_next(counter_tip: str) -> int:
    result = checked_git(
        PROGRAM,
        f"counter branch {COUNTER_BRANCH} carries no NEXT file",
        "show",
        f"{counter_tip}:NEXT",
    )
    text = result.stdout.strip()
    if not text.isdigit():
        die(PROGRAM, f"counter branch NEXT is not a number: {text!r}")
    return int(text)


def build_claim_commit(parent: str | None, claimed: int) -> str:
    """Assemble the bump commit (NEXT := claimed + 1) with plumbing only."""
    blob = checked_git(
        PROGRAM,
        "cannot write NEXT blob",
        "hash-object",
        "-w",
        "--stdin",
        input_text=f"{claimed + 1:04d}\n",
    )
    tree = checked_git(
        PROGRAM,
        "cannot assemble claim tree",
        "mktree",
        input_text=f"100644 blob {blob.stdout.strip()}\tNEXT\n",
    )
    args = ["commit-tree", tree.stdout.strip()]
    if parent is not None:
        args += ["-p", parent]
    args += ["-m", claim_subject(claimed)]
    commit = checked_git(PROGRAM, "cannot create claim commit", *args)
    return commit.stdout.strip()


def mint(remote: str) -> int:
    """Claim one number: fetch, compute, build, CAS push, print number + SHA."""
    last_claimed, last_error = 0, ""
    for _attempt in range(1, MAX_ATTEMPTS + 1):
        main_max = max_adr_prefix(fetch_main(remote))
        counter_tip = fetch_counter(remote)
        next_value = read_next(counter_tip) if counter_tip is not None else 0
        # Self-heal: never mint at or below main_max (even when NEXT lags),
        # and never below FLOOR. On init (no counter branch) this lands on
        # max(main_max + 1, FLOOR).
        claimed = max(next_value, main_max + 1, FLOOR)
        commit = build_claim_commit(counter_tip, claimed)
        # Unchecked on purpose: a rejected push is the CAS retry signal.
        result = git("push", remote, f"{commit}:refs/heads/{COUNTER_BRANCH}")
        if result.returncode == 0:
            # Two-line contract: the claimed number (unchanged) then the
            # SHA of the claim commit this push just landed on the branch.
            print(f"{claimed:04d}")
            print(commit)
            return 0
        last_claimed = claimed
        last_error = (result.stderr or result.stdout).strip()
    die(
        PROGRAM,
        f"conflict claiming ADR number {last_claimed:04d}: "
        f"{MAX_ATTEMPTS} push attempts to {COUNTER_BRANCH} were rejected "
        f"(another mint is racing us); last rejection: {last_error}",
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Claim the next ADR number from the shared counter branch "
            f"{COUNTER_BRANCH} and print it zero-padded (e.g. 0243) on stdout, "
            "followed by the SHA of the claim commit pushed to that branch "
            "on the second line. The caller names docs/adr/NNNN-<slug>.md "
            "itself; the mint is recorded on the branch as a claim commit "
            "whose subject names the claimed number."
        ),
        epilog=(
            "Output on success (stdout):\n"
            "  line 1  the claimed number, zero-padded to four digits "
            "(e.g. 0243)\n"
            "  line 2  the 40-hex SHA of the claim commit pushed to "
            f"refs/heads/{COUNTER_BRANCH}\n"
            "\n"
            "Example:\n"
            "  python3 scripts/adr_mint.py --remote origin\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--remote",
        default="origin",
        metavar="NAME",
        help="git remote hosting the counter branch (default: origin)",
    )
    args = parser.parse_args()
    if args.remote.startswith("-"):
        parser.error(f"--remote must not begin with '-': {args.remote!r}")
    anchor_repo_root(PROGRAM)
    return mint(args.remote)


if __name__ == "__main__":
    sys.exit(main())
