"""Tests for scripts/check_adr_numbers.py.

Each test builds a scratch git repo in a temp dir through the shared
adr_fixtures.AdrFixture (the same construction the mint suite uses): a
bare `remote.git` as `origin`, one work repo whose `main` carries the
seeded ADR, and a `feature` branch forked from it. The remote carries the
ledger branch (adr_fixtures.COUNTER_BRANCH) once a test claims a number
with AdrFixture.claim — a claim commit pushed exactly as
scripts/adr_mint.py pushes one — and the checker proves each added ADR's
`Claim: <sha>` header against that branch. No network; commits use the
shared deterministic GIT_AUTHOR_*/GIT_COMMITTER_* environment
(adr_fixtures.GIT_ENV), and the ledger machinery
(adr_fixtures.COUNTER_BRANCH/REF, push_counter_commit, ref_tip), the
asserting git runner, and the run-script-under-test helper all come from
adr_fixtures.py.
"""

import tempfile
import unittest
from pathlib import Path

from adr_fixtures import (
    AdrFixture,
    COUNTER_BRANCH,
    GIT_ENV,
    run_git,
)


def adr_body(number: str, claim: str | None, context: str = "fixture") -> str:
    """An ADR file body: title, Status, optional Claim header, one section."""
    lines = [f"# ADR-{number}: fixture decision", "", "Status: Proposed", ""]
    if claim is not None:
        lines += [f"Claim: {claim}", ""]
    lines += ["## Context", "", context, ""]
    return "\n".join(lines)


class FixtureTestBase(unittest.TestCase):
    """One shared fixture repo per test, seeded with the existing ADR."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.fx = AdrFixture(Path(self._tmp.name), ("0001-existing-decision.md",))


class CheckAdrNumbersTests(FixtureTestBase):
    """The number conflict checks: base collision and intra-PR duplicates."""

    def test_added_prefix_already_on_base_fails(self) -> None:
        self.fx.commit("feature", {"docs/adr/0001-a-second-decision.md": "# clash\n"})
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("0001", result.stderr)
        self.assertIn("scripts/adr_mint.py", result.stderr)

    def test_two_added_files_sharing_a_prefix_fail(self) -> None:
        self.fx.commit(
            "feature",
            {
                "docs/adr/0007-first-title.md": "# one\n",
                "docs/adr/0007-second-title.md": "# two\n",
            },
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("0007", result.stderr)
        self.assertIn("scripts/adr_mint.py", result.stderr)

    def commit_rename(self, body: str) -> None:
        """On feature: add 0001-renamed-slug.md, delete 0001-existing, commit."""
        self.fx.commit(
            "feature",
            {
                "docs/adr/0001-renamed-slug.md": body,
                "docs/adr/0001-existing-decision.md": None,
            },
        )

    def test_same_number_rename_passes(self) -> None:
        claim = self.fx.claim("0001")
        self.commit_rename(adr_body("0001", claim))
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("1 re-slotted rename(s)", result.stdout)

    def test_a_similarity_detected_rename_decomposes_into_add_and_delete(self) -> None:
        # git would report a pure `git mv` as one R record; the checker's
        # --no-renames decomposes it into the A+D pair the re-slot rule and
        # the claim proof read. The moved file carries no Claim header, so
        # the proof — not the conflict check — is what fails it.
        self.fx.claim("0300")  # the ledger branch must exist for the proof
        run_git(self.fx.work, "checkout", "-q", "feature", env=GIT_ENV)
        run_git(
            self.fx.work, "mv",
            "docs/adr/0001-existing-decision.md",
            "docs/adr/0001-moved-slug.md",
            env=GIT_ENV,
        )
        run_git(
            self.fx.work, "commit", "-q", "-m", "move 0001 under a new slug",
            env=GIT_ENV,
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0001-moved-slug.md", result.stderr)
        self.assertIn("Claim:", result.stderr)

    def test_rename_without_a_claim_still_fails_the_claim_proof(self) -> None:
        self.fx.claim("0001")
        self.commit_rename(adr_body("0001", None))
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0001-renamed-slug.md", result.stderr)
        self.assertIn("Claim:", result.stderr)

    def test_clean_addition_passes(self) -> None:
        second = self.fx.claim("0002")
        third = self.fx.claim("0003")
        self.fx.commit(
            "feature",
            {
                "docs/adr/0002-a-fresh-decision.md": adr_body("0002", second),
                "docs/adr/0003-another-fresh-one.md": adr_body("0003", third),
            },
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_no_adr_additions_passes(self) -> None:
        self.fx.commit("feature", {"README.md": "touch\n"})
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_unresolvable_base_is_a_usage_error(self) -> None:
        result = self.fx.run("--base", "no-such-ref")
        self.assertEqual(result.returncode, 2)
        self.assertIn("no-such-ref", result.stderr)

    def test_run_from_subdirectory_still_detects_conflict(self) -> None:
        self.fx.commit("feature", {"docs/adr/0001-a-second-decision.md": "# clash\n"})
        result = self.fx.run("--base", "main", cwd=self.fx.work / "docs")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("0001", result.stderr)
        self.assertIn("scripts/adr_mint.py", result.stderr)

    def test_added_adr_with_tab_in_name_still_conflicts(self) -> None:
        # Without -z, git C-quotes this path ("...0001-with\ttab.md") and the
        # addition evades the ADR_NAME match entirely.
        self.fx.commit("feature", {"docs/adr/0001-with\ttab.md": "# clash\n"})
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("0001", result.stderr)

    def test_base_beginning_with_dash_is_a_usage_error(self) -> None:
        result = self.fx.run("--base=-evil")
        self.assertEqual(result.returncode, 2)
        self.assertIn("--base", result.stderr)
        self.assertIn("-evil", result.stderr)


class ClaimProofTests(FixtureTestBase):
    """The claim proof: an added number must be minted on the ledger."""

    def test_properly_claimed_adrs_pass(self) -> None:
        first = self.fx.claim("0300")
        second = self.fx.claim("0301")
        self.fx.commit(
            "feature",
            {
                "docs/adr/0300-claimed-first.md": adr_body("0300", first),
                "docs/adr/0301-claimed-second.md": adr_body("0301", second),
            },
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_claim_header_fails(self) -> None:
        self.fx.claim("0300")
        self.fx.commit(
            "feature", {"docs/adr/0300-no-claim.md": adr_body("0300", None)}
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0300-no-claim.md", result.stderr)
        self.assertIn("0300", result.stderr)
        self.assertIn("Claim:", result.stderr)
        self.assertIn("scripts/adr_mint.py", result.stderr)

    def test_claim_outside_the_header_block_fails(self) -> None:
        sha = self.fx.claim("0300")
        late = "\n".join(
            [
                "# ADR-0300: fixture decision",
                "",
                "Status: Proposed",
                "",
                "## Context",
                "",
                f"Claim: {sha}",
                "",
            ]
        )
        self.fx.commit("feature", {"docs/adr/0300-late-claim.md": late})
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0300-late-claim.md", result.stderr)
        self.assertIn("Claim:", result.stderr)

    def test_claim_sha_off_the_ledger_fails(self) -> None:
        self.fx.claim("0300")
        stray = run_git(
            self.fx.work, "rev-parse", "main", env=GIT_ENV
        ).stdout.strip()
        self.fx.commit(
            "feature", {"docs/adr/0300-stray-claim.md": adr_body("0300", stray)}
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0300-stray-claim.md", result.stderr)
        self.assertIn(stray, result.stderr)
        self.assertIn(COUNTER_BRANCH, result.stderr)
        self.assertIn("scripts/adr_mint.py", result.stderr)

    def test_claim_sha_naming_no_commit_fails(self) -> None:
        self.fx.claim("0300")
        fake = "f" * 40
        self.fx.commit(
            "feature", {"docs/adr/0300-fake-claim.md": adr_body("0300", fake)}
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0300-fake-claim.md", result.stderr)
        self.assertIn(fake, result.stderr)

    def test_claim_subject_for_another_number_fails(self) -> None:
        sha = self.fx.claim("0300")
        self.fx.commit(
            "feature", {"docs/adr/0301-wrong-claim.md": adr_body("0301", sha)}
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0301-wrong-claim.md", result.stderr)
        self.assertIn("claim 0300", result.stderr)
        self.assertIn("claim 0301", result.stderr)
        self.assertIn("scripts/adr_mint.py", result.stderr)

    def test_modified_existing_adr_is_grandfathered(self) -> None:
        # ADR-0301: the checker only examines the ADRs a change adds, so a
        # modified ADR without a Claim header anywhere is never re-checked.
        self.fx.commit(
            "feature",
            {"docs/adr/0001-existing-decision.md": "# existing, amended\n"},
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("0 ADR file(s) added", result.stdout)

    def test_absent_ledger_fails_every_added_adr(self) -> None:
        self.assertIsNone(self.fx.counter_tip())
        self.fx.commit(
            "feature",
            {
                "docs/adr/0300-no-ledger-one.md": adr_body("0300", "a" * 40),
                "docs/adr/0301-no-ledger-two.md": adr_body("0301", "b" * 40),
            },
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0300-no-ledger-one.md", result.stderr)
        self.assertIn("docs/adr/0301-no-ledger-two.md", result.stderr)
        self.assertIn(COUNTER_BRANCH, result.stderr)
        self.assertIn("scripts/adr_mint.py", result.stderr)


class PublicRecordTests(FixtureTestBase):
    """The public-record content check (AGENTS.md §1): no internal
    references on lines the change adds under docs/adr/."""

    def seed_reference_on_main(self, body: str) -> None:
        """Put a ref-carrying ADR on main, then re-point feature at it.

        Committed through `commit("main", ...)` and `branch -f` so the
        reference sits at the merge base the checker diffs against —
        exactly the shape of the ADRs already in the repository that
        carry Linear ids from before the public-record rule.
        """
        self.fx.commit("main", {"docs/adr/0002-legacy-decision.md": body})
        run_git(self.fx.work, "branch", "-f", "feature", "main", env=GIT_ENV)

    def assert_added_adr_reference_fails(
        self, name: str, context: str, expected: str
    ) -> None:
        """Claim 0300, add one ADR whose Context carries the reference,
        and assert the checker fails naming the file and the token."""
        claim = self.fx.claim("0300")
        self.fx.commit(
            "feature",
            {f"docs/adr/{name}": adr_body("0300", claim, context=context)},
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(f"docs/adr/{name}", result.stderr)
        self.assertIn(expected, result.stderr)
        self.assertIn("AGENTS.md", result.stderr)

    def test_added_adr_with_a_labs_reference_fails(self) -> None:
        self.assert_added_adr_reference_fails(
            "0300-tracked-decision.md", "tracked as LABS-123", "LABS-123"
        )

    def test_added_adr_with_a_linear_app_link_fails(self) -> None:
        # No LABS-N id in the link: this isolates the bare linear.app
        # leg of the pattern from the ticket-id leg.
        self.assert_added_adr_reference_fails(
            "0300-linked-decision.md",
            "see https://linear.app/neo4j/issue/PROJ-411",
            "linear.app",
        )

    def test_added_adr_with_a_cip_reference_fails(self) -> None:
        self.assert_added_adr_reference_fails(
            "0300-cip-decision.md", "per CIP-274", "CIP-274"
        )

    def test_glob_character_path_adr_is_scanned(self) -> None:
        # A pathspec is a glob unless made literal: an ADR named with a
        # bracket character class must be scanned as itself, never
        # silently matched against a different file (or nothing).
        self.assert_added_adr_reference_fails(
            "0300-[x]-decision.md", "tracked as LABS-3", "LABS-3"
        )

    def test_reference_behind_a_unicode_separator_is_scanned(self) -> None:
        # str.splitlines would tear the added line at U+2028 and drop
        # the tail unread; the scan splits on \n only, so the whole
        # diff line is searched.
        self.assert_added_adr_reference_fails(
            "0300-separated-decision.md", f"see{chr(0x2028)}LABS-4", "LABS-4"
        )

    def test_nul_containing_adr_is_scanned(self) -> None:
        # git classifies a NUL-bearing blob as binary and emits no hunks
        # unless told --text; the scan reads it like any other ADR.
        self.assert_added_adr_reference_fails(
            "0300-binary-decision.md", "see\x00 LABS-6", "LABS-6"
        )

    def test_modification_adding_a_reference_fails(self) -> None:
        # A modified — not added — ADR: the content check reads the diff,
        # so a "Later references" note smuggling in an internal reference
        # fails even though no number or claim rule applies.
        self.fx.commit(
            "feature",
            {
                "docs/adr/0001-existing-decision.md": (
                    "# ADR-0001: existing, amended\n\ntracked as LABS-42\n"
                )
            },
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("docs/adr/0001-existing-decision.md", result.stderr)
        self.assertIn("LABS-42", result.stderr)
        self.assertIn("AGENTS.md", result.stderr)

    def test_preexisting_reference_lines_are_grandfathered(self) -> None:
        # Only added lines count: editing an ADR that already carries an
        # internal reference (pre-rule history) never fails the edit.
        self.seed_reference_on_main(
            adr_body("0002", None, context="tracked as LABS-7")
        )
        amended = (
            adr_body("0002", None, context="tracked as LABS-7")
            + "\n## Later references\n\n- superseded by ADR-0003\n"
        )
        self.fx.commit("feature", {"docs/adr/0002-legacy-decision.md": amended})
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_removing_a_reference_line_passes(self) -> None:
        # Removing an internal reference is a fix, never a failure.
        self.seed_reference_on_main(
            adr_body("0002", None, context="tracked as LABS-7")
        )
        self.fx.commit(
            "feature",
            {"docs/adr/0002-legacy-decision.md": adr_body("0002", None)},
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_deleting_a_ref_carrying_adr_alongside_an_edit_passes(self) -> None:
        # A deletion's +++ /dev/null header is a header, never an added
        # line of the file before it: deleting the ref-carrying ADR while
        # editing another one stays quiet.
        self.seed_reference_on_main(
            adr_body("0002", None, context="tracked as LABS-7")
        )
        self.fx.commit(
            "feature",
            {
                "docs/adr/0001-existing-decision.md": "# ADR-0001: amended\n",
                "docs/adr/0002-legacy-decision.md": None,
            },
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pure_re_slot_of_a_ref_carrying_adr_passes(self) -> None:
        # The number rule supports re-slotting: delete the old file, add
        # a new one under the same number. The content rule shares that
        # policy — a pure rename of an ADR already carrying a
        # grandfathered reference contributes no added lines.
        claim = self.fx.claim("0002")
        body = adr_body("0002", claim, context="tracked as LABS-7")
        self.seed_reference_on_main(body)
        self.fx.commit(
            "feature",
            {
                "docs/adr/0002-renamed-decision.md": body,
                "docs/adr/0002-legacy-decision.md": None,
            },
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_edited_re_slot_adding_a_reference_fails(self) -> None:
        # An edited re-slot diffs pre-image against post-image: the
        # grandfathered LABS-7 line is context, only the LABS-8 edit is
        # an addition and fails.
        claim = self.fx.claim("0002")
        old_body = adr_body("0002", claim, context="tracked as LABS-7")
        self.seed_reference_on_main(old_body)
        new_body = old_body + "\nand LABS-8 too\n"
        self.fx.commit(
            "feature",
            {
                "docs/adr/0002-renamed-decision.md": new_body,
                "docs/adr/0002-legacy-decision.md": None,
            },
        )
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("LABS-8", result.stderr)
        self.assertNotIn("LABS-7", result.stderr)

    def test_quoted_path_adr_with_a_reference_fails(self) -> None:
        # A path git C-quotes in the diff header (+++ "b/...") is still
        # scanned: a tab-named ADR must not silently escape the
        # public-record check.
        self.assert_added_adr_reference_fails(
            "0300-with\ttab.md", "tracked as LABS-5", "LABS-5"
        )

    def test_body_line_mimicking_a_file_header_is_scanned(self) -> None:
        # An added line whose content starts with '++ b/' reaches the
        # diff as '+++ b/…' — a byte-for-byte file-header lookalike. It
        # must be read as content, never classified as a header (the
        # round-1 stellar/security evasion finding).
        claim = self.fx.claim("0300")
        body = adr_body("0300", claim) + "++ b/docs/adr/LABS-9\n"
        self.fx.commit("feature", {"docs/adr/0300-header-mimic.md": body})
        result = self.fx.run("--base", "main")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("LABS-9", result.stderr)


if __name__ == "__main__":
    unittest.main()
