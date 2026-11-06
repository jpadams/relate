"""Tests for scripts/version_tokens.py, the release sweep's token rule.

`SHAPES` is the contract `scripts/release-bump.sh` relies on: one row per token
shape the sweep has to classify, spelled `(name, before, after, rewritten
tokens)`. `ReleaseSweepShapes` drives the same rows through the CLI subcommands
the shell actually calls — a module/CLI disagreement would leave the sweep and
its self-check describing different rules.
"""

import argparse
import io
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path

from script_loader import ROOT, load

vt = load("version_tokens", ROOT / "scripts" / "version_tokens.py")

#: (name, before, after, rewritten tokens) — one row per token shape the
#: contract classifies, including the dotted quad that must never move.
SHAPES = [
    (
        "dotted quad untouched",
        "binds 0.0.0.0 and 127.0.0.1",
        "binds 0.0.0.0 and 127.0.0.1",
        [],
    ),
    (
        "sentence-final token",
        "Shipped as Relate 0.13.0.",
        "Shipped as Relate 9.9.9.",
        ["0.13.0"],
    ),
    (
        "dotted filename suffix",
        "curl -O relate-0.13.0.tar.gz",
        "curl -O relate-9.9.9.tar.gz",
        ["0.13.0"],
    ),
    (
        "two tokens on one line",
        "supported: 0.13.0,0.12.0",
        "supported: 9.9.9,9.9.9",
        ["0.13.0", "0.12.0"],
    ),
    (
        "two tokens separated by words",
        "relate 0.13.0 pairs with the 0.12.0 driver",
        "relate 9.9.9 pairs with the 9.9.9 driver",
        ["0.13.0", "0.12.0"],
    ),
    (
        "prerelease suffix",
        "candidate 0.13.0-rc.1 stays a candidate",
        "candidate 9.9.9-rc.1 stays a candidate",
        ["0.13.0"],
    ),
]

#: Digit/dot runs the rule must classify as something other than a version
#: token, and therefore never rewrite.
NON_TOKENS = [
    ("dotted quad", "0.0.0.0"),
    ("four-component build number", "0.13.0.0"),
    ("empty component", "1.2..3"),
    ("bare number", "8080"),
    ("dot-adjacent fragment", ".1.2"),
]


class TestTokenRule(unittest.TestCase):
    """`rewrite`/`stale`: the rule itself, one table row per shape."""

    def test_shapes_table(self):
        for name, before, after, tokens in SHAPES:
            with self.subTest(shape=name):
                updated, rewritten, _left_alone = vt.rewrite(before, "9.9.9")
                self.assertEqual(updated, after)
                self.assertEqual(rewritten, tokens)

    def test_non_tokens_are_never_rewritten(self):
        for name, run in NON_TOKENS:
            with self.subTest(shape=name):
                updated, rewritten, left_alone = vt.rewrite(f"x {run} y", "9.9.9")
                self.assertEqual(updated, f"x {run} y")
                self.assertEqual(rewritten, [])
                self.assertEqual(left_alone, [run])

    def test_dotted_quad_does_not_hide_a_stale_token(self):
        self.assertEqual(vt.stale("binds 0.0.0.0 at 0.12.0.", "9.9.9"), ["0.12.0"])

    def test_stale_is_silent_on_the_expected_version(self):
        self.assertEqual(vt.stale("Relate v9.9.9.\n", "9.9.9"), [])

    def test_prerelease_tail_survives_and_is_not_stale(self):
        text = vt.rewrite("candidate 9.9.9-rc.1", "9.9.9")[0]
        self.assertEqual(text, "candidate 9.9.9-rc.1")
        self.assertEqual(vt.stale(text, "9.9.9"), [])

    def test_rewrite_leaves_trailing_punctuation_in_place(self):
        self.assertEqual(vt.rewrite("v0.13.0..", "9.9.9")[0], "v9.9.9..")

    def test_the_bump_version_and_the_token_rule_share_one_shape(self):
        # One spelling of `X.Y.Z` backs both boundaries: what `--to`/`--expect`
        # accept and what the sweep rewrites.
        for candidate in ["0.13.0", "1.2.3"]:
            with self.subTest(candidate=candidate):
                self.assertEqual(vt._version(candidate), candidate)
                self.assertTrue(vt.is_version_token(candidate))
        for candidate in ["0.13", "0.13.0.0", "v0.13.0"]:
            with self.subTest(candidate=candidate):
                with self.assertRaises(argparse.ArgumentTypeError):
                    vt._version(candidate)
                self.assertFalse(vt.is_version_token(candidate))


class ReleaseSweepShapes(unittest.TestCase):
    """The CLI subcommands `scripts/release-bump.sh` calls, on the same rows."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = Path(self._tmp.name)

    def write(self, name: str, body: str) -> Path:
        path = self.dir / name
        path.write_text(body, encoding="utf-8")
        return path

    def _main(self, argv: list[str]) -> tuple[int, str]:
        """Run `vt.main` under a captured stderr: `(exit code, output)`.

        The scaffold every CLI test in this class rides — each test states
        only its scenario and its assertions.
        """
        captured = io.StringIO()
        with redirect_stderr(captured):
            code = vt.main(argv)
        return code, captured.getvalue()

    def test_rewrite_then_check_agrees_on_every_shape(self):
        for name, before, after, _tokens in SHAPES:
            with self.subTest(shape=name):
                path = self.write("shape.md", before)
                code, _ = self._main(["rewrite", "--to", "9.9.9", str(path)])
                self.assertEqual(code, 0)
                self.assertEqual(path.read_text(encoding="utf-8"), after)
                code, _ = self._main(["check", "--expect", "9.9.9", str(path)])
                self.assertEqual(code, 0)

    def test_check_reports_a_stale_token_and_exits_one(self):
        path = self.write("stale.md", "Shipped 9.9.9.\nThen 0.12.0.\n")
        code, output = self._main(["check", "--expect", "9.9.9", str(path)])
        self.assertEqual(code, 1)
        self.assertIn("version token(s) that are not 9.9.9: 0.12.0", output)

    def test_check_does_not_call_a_dotted_quad_stale(self):
        path = self.write("quad.md", "binds 0.0.0.0\n")
        code, _ = self._main(["check", "--expect", "9.9.9", str(path)])
        self.assertEqual(code, 0)

    def test_check_names_every_stale_file_and_exits_one(self):
        first = self.write("first.md", "0.13.0\n")
        second = self.write("second.md", "0.12.0\n")
        code, output = self._main(["check", "--expect", "9.9.9", str(first), str(second)])
        self.assertEqual(code, 1)
        self.assertIn(str(first), output)
        self.assertIn(str(second), output)

    def test_rewrite_processes_every_file_around_an_unreadable_one(self):
        before = self.write("before.md", "Shipped 0.13.0.\n")
        after = self.write("after.md", "Relate v0.13.0\n")
        missing = self.dir / "missing.md"
        code, output = self._main(
            ["rewrite", "--to", "9.9.9", str(before), str(missing), str(after)]
        )
        self.assertEqual(code, 2)
        self.assertIn(f"version_tokens: {missing}: cannot read:", output)
        self.assertEqual(before.read_text(encoding="utf-8"), "Shipped 9.9.9.\n")
        self.assertEqual(after.read_text(encoding="utf-8"), "Relate v9.9.9\n")

    def test_check_processes_every_file_around_an_unreadable_one(self):
        stale_file = self.write("stale.md", "0.13.0\n")
        missing = self.dir / "missing.md"
        code, output = self._main(
            ["check", "--expect", "9.9.9", str(missing), str(stale_file)]
        )
        self.assertEqual(code, 2)
        self.assertIn(f"version_tokens: {missing}: cannot read:", output)
        self.assertIn("version token(s) that are not 9.9.9: 0.13.0", output)

    def test_rewrite_reports_the_runs_it_left_alone(self):
        path = self.write("quad.md", "binds 0.0.0.0\n")
        code, output = self._main(["rewrite", "--to", "9.9.9", str(path)])
        self.assertEqual(code, 0)
        self.assertIn("skipped non-version dotted run(s): 0.0.0.0", output)
        self.assertEqual(path.read_text(encoding="utf-8"), "binds 0.0.0.0\n")

    def test_rewrite_does_not_report_bare_integers(self):
        path = self.write("ports.md", "binds 8080 and 7474\n")
        code, output = self._main(["rewrite", "--to", "9.9.9", str(path)])
        self.assertEqual(code, 0)
        self.assertEqual(output, "")

    def test_rewrite_rejects_a_non_bare_version(self):
        path = self.write("doc.md", "0.13.0\n")
        with self.assertRaises(SystemExit) as raised:
            self._main(["rewrite", "--to", "v9.9.9", str(path)])
        self.assertEqual(raised.exception.code, 2)
        self.assertEqual(path.read_text(encoding="utf-8"), "0.13.0\n")


if __name__ == "__main__":
    unittest.main()
