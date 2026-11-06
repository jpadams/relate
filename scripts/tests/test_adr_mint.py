"""Fixture-repo tests for scripts/adr_mint.py.

Each fixture is a temp dir holding a bare repo (the remote) plus a work
repo whose main carries a seeded docs/adr — LOW_ADRS (main_max 42, below
the mint floor) or HIGH_ADRS (main_max 300, at it) — alongside decoy names
that must not count (README.md, a five-digit prefix, a non-.md file). The
fixture construction is shared with the checker suite
(adr_fixtures.AdrFixture, which seeds the work repo directly — no seed
repo plus clone); every git command runs with the shared deterministic
identity environment (adr_fixtures.GIT_ENV); the remote and the work repo
also carry a repo-config identity, which covers the commits created
without GIT_ENV in the environment — the script under test commits in the
work repo, and the remote's pre-receive hook commits in the remote. The
counter branch is seeded and probed through the shared adr_fixtures
machinery (push_counter_commit, ref_tip, COUNTER_BRANCH/REF), so this
suite and the checker suite cannot drift on the ledger shape. The remote
is a local path (no network), and the script under test runs as a
subprocess in the work repo, exactly as a caller would run it. A
successful mint prints two lines — the claimed number, then the SHA of
the claim commit it pushed — so `mint_ok` returns line 1, and the
output-contract tests read the bare remote's tip to pin line 2.

Run directly, like every scripts/tests/ suite: python3 scripts/tests/test_adr_mint.py
"""

from __future__ import annotations

import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

from adr_fixtures import (
    AdrFixture,
    GIT_ENV,
    MINT_SCRIPT,
    REF,
    ROOT,
    run_git,
    run_script,
)


# Fixtures: LOW sits below the mint floor (main_max 42), HIGH sits at it
# (main_max 300). The 0300 floor exists because in-flight branches may hold
# 0243-0299 grabbed under the old list-the-directory rule.
LOW_ADRS = ("0001-first.md", "0007-seventh.md", "0042-the-answer.md")
HIGH_ADRS = (*LOW_ADRS, "0300-three-hundred.md")


class AdrMintTestBase(unittest.TestCase):
    adrs: tuple[str, ...] = LOW_ADRS

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.base = Path(self._tmp.name)
        self.fx = AdrFixture(self.base, self.adrs)

    def mint_ok(self, *args: str, cwd: Path | None = None) -> str:
        """Mint once; assert success and return the printed number (line 1)."""
        result = self.fx.mint(*args, cwd=cwd)
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        self.assertTrue(lines, "mint succeeded but printed nothing")
        return lines[0]


class AboveFloorTests(AdrMintTestBase):
    """main_max = 0300: every mint lands above the floor."""

    adrs = HIGH_ADRS

    def test_successive_mints_are_strictly_increasing(self) -> None:
        claimed = [self.mint_ok() for _ in range(3)]
        self.assertEqual(claimed, ["0301", "0302", "0303"])
        self.assertEqual(
            self.fx.counter_log(), ["claim 0303", "claim 0302", "claim 0301"]
        )
        self.assertEqual(self.fx.counter_next(), "0304")

    def test_remote_bump_between_fetch_and_push_is_retried(self) -> None:
        self.assertEqual(self.mint_ok(), "0301")  # counter branch now exists
        flag = self.base / "race-done"
        # First push after this races us: the hook claims 0302 itself (a
        # competing mint landing between our fetch and our push), rejects our
        # push, and lets the retry through. The hook runs inside the bare
        # remote's pre-receive, so it hand-rolls the same plumbing in shell
        # instead of calling adr_fixtures.push_counter_commit. Object writes
        # must escape the push quarantine, so the hook unsets the quarantine
        # environment.
        self.fx.install_hook(
            "pre-receive",
            textwrap.dedent(
                f"""\
                #!/bin/sh
                if [ -f "{flag}" ]; then
                  exit 0
                fi
                touch "{flag}"
                unset GIT_QUARANTINE_PATH GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES
                blob=$(printf '0303\\n' | git hash-object -w --stdin)
                tree=$(printf '100644 blob %s\\tNEXT\\n' "$blob" | git mktree)
                parent=$(git rev-parse {REF})
                commit=$(git commit-tree "$tree" -p "$parent" -m "claim 0302")
                git update-ref {REF} "$commit"
                echo "racing mint claimed 0302" >&2
                exit 1
                """
            ),
        )
        self.assertEqual(self.mint_ok(), "0303")
        self.assertEqual(
            self.fx.counter_log(), ["claim 0303", "claim 0302", "claim 0301"]
        )

    def test_missing_counter_branch_is_initialized_from_main_max(self) -> None:
        self.assertIsNone(self.fx.counter_tip())
        self.assertEqual(self.mint_ok(), "0301")
        self.assertEqual(self.fx.counter_next(), "0302")
        self.assertEqual(self.fx.counter_log(), ["claim 0301"])
        parents = run_git(
            self.fx.remote, "log", "--format=%P", REF, env=GIT_ENV
        ).stdout.strip()
        self.assertEqual(parents, "")  # the init commit is a root commit

    def test_lagging_next_self_heals_to_main_max_plus_one(self) -> None:
        self.fx.write_counter(5, "claim 0004")
        self.assertEqual(self.mint_ok(), "0301")  # not 0005, not the floor
        self.assertEqual(self.fx.counter_next(), "0302")

    def test_subdirectory_invocation_self_heals_to_main_max_plus_one(self) -> None:
        # From a subdirectory a cwd-relative docs/adr pathspec matches
        # nothing, making main_max 0 — the mint would land on the 0300
        # floor instead of 0301.
        self.fx.write_counter(5, "claim 0004")
        subdir = self.fx.work / "docs" / "adr"
        self.assertEqual(self.mint_ok(cwd=subdir), "0301")
        self.assertEqual(self.fx.counter_next(), "0302")

    def test_remote_beginning_with_dash_is_rejected(self) -> None:
        result = self.fx.mint("--remote=-evil")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--remote", result.stderr)
        self.assertIn("-evil", result.stderr)

    def test_ledger_commit_message_records_the_claimed_number(self) -> None:
        self.assertEqual(self.mint_ok(), "0301")
        subject = run_git(
            self.fx.remote, "log", "-1", "--format=%s", REF, env=GIT_ENV
        ).stdout.strip()
        self.assertEqual(subject, "claim 0301")
        self.assertEqual(self.mint_ok(), "0302")
        self.assertEqual(self.fx.counter_log(), ["claim 0302", "claim 0301"])


class FloorTests(AdrMintTestBase):
    """main_max = 42: the 0300 floor governs every mint."""

    adrs = LOW_ADRS

    def test_floor_pins_minimum_0300(self) -> None:
        # NEXT and main_max + 1 both below 0300: the floor wins.
        self.fx.write_counter(1, "claim 0000")
        self.assertEqual(self.mint_ok(), "0300")
        self.assertEqual(self.fx.counter_next(), "0301")
        self.assertEqual(self.fx.counter_log(), ["claim 0300", "claim 0000"])
        # and from there mints climb off the floor
        self.assertEqual(self.mint_ok(), "0301")

    def test_missing_counter_branch_initializes_at_the_floor(self) -> None:
        self.assertIsNone(self.fx.counter_tip())
        self.assertEqual(self.mint_ok(), "0300")  # max(main_max + 1, floor)
        self.assertEqual(self.fx.counter_next(), "0301")
        self.assertEqual(self.fx.counter_log(), ["claim 0300"])

    def test_dirty_worktree_and_branch_are_untouched(self) -> None:
        tracked = self.fx.work / "docs" / "adr" / "0001-first.md"
        tracked.write_text("dirty\n")
        (self.fx.work / "scratch.txt").write_text("untracked\n")
        before = run_git(self.fx.work, "status", "--porcelain", env=GIT_ENV).stdout
        head_before = run_git(self.fx.work, "rev-parse", "HEAD", env=GIT_ENV).stdout.strip()
        self.assertEqual(self.mint_ok(), "0300")
        self.assertEqual(
            run_git(self.fx.work, "status", "--porcelain", env=GIT_ENV).stdout, before
        )
        self.assertEqual(
            run_git(self.fx.work, "rev-parse", "HEAD", env=GIT_ENV).stdout.strip(),
            head_before,
        )
        self.assertEqual(
            run_git(self.fx.work, "branch", "--show-current", env=GIT_ENV).stdout.strip(),
            "main",
        )
        self.assertEqual(tracked.read_text(), "dirty\n")

    def test_always_rejecting_remote_exits_one_after_ten_attempts(self) -> None:
        attempts = self.base / "attempts"
        self.fx.install_hook(
            "pre-receive",
            textwrap.dedent(
                f"""\
                #!/bin/sh
                echo attempt >> "{attempts}"
                echo "pre-receive: no ADR numbers for you" >&2
                exit 1
                """
            ),
        )
        result = self.fx.mint()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("conflict", result.stderr)
        self.assertIn("0300", result.stderr)
        self.assertEqual(len(attempts.read_text().splitlines()), 10)
        self.assertIsNone(self.fx.counter_tip())


class ClaimCommitOutputTests(AdrMintTestBase):
    """Success prints the number, then the SHA of the pushed claim commit."""

    adrs = HIGH_ADRS

    def test_stdout_prints_number_then_claim_commit_sha(self) -> None:
        result = self.fx.mint()
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        self.assertEqual(len(lines), 2, result.stdout)
        self.assertEqual(lines[0], "0301")
        self.assertRegex(lines[1], r"^[0-9a-f]{40}$")
        # Line 2 is the commit the push just landed on the remote: the
        # bare remote's counter-branch tip is that commit.
        self.assertEqual(lines[1], self.fx.counter_tip())

    def test_each_mint_prints_its_own_claim_commit(self) -> None:
        first = self.fx.mint()
        self.assertEqual(first.returncode, 0, first.stderr)
        first_tip = self.fx.counter_tip()
        second = self.fx.mint()
        self.assertEqual(second.returncode, 0, second.stderr)
        lines = second.stdout.splitlines()
        self.assertEqual(len(lines), 2, second.stdout)
        self.assertEqual(lines[0], "0302")
        self.assertEqual(lines[1], self.fx.counter_tip())
        self.assertNotEqual(lines[1], first_tip)


class AdrMintHelpTests(unittest.TestCase):
    def test_help_prints_usage_flags_and_invocation(self) -> None:
        result = run_script(MINT_SCRIPT, "--help", cwd=ROOT)
        self.assertEqual(result.returncode, 0)
        self.assertIn("usage:", result.stdout)
        self.assertIn("--remote", result.stdout)
        self.assertIn("origin", result.stdout)
        self.assertIn("python3 scripts/adr_mint.py", result.stdout)

    def test_help_documents_the_two_line_output(self) -> None:
        result = run_script(MINT_SCRIPT, "--help", cwd=ROOT)
        self.assertEqual(result.returncode, 0)
        self.assertIn("line 1", result.stdout)
        self.assertIn("line 2", result.stdout)
        self.assertIn("40-hex SHA", result.stdout)


if __name__ == "__main__":
    unittest.main()
