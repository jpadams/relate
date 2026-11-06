"""The documented Python examples run end to end (AGENTS.md 2: examples are tests).

The list is derived from `docs/api/python/examples/`, so a new example is
picked up automatically — never a hand-maintained mirror.
"""

import pathlib
import runpy

import pytest

# parents[4] is the repo root: tests -> python -> relate-python -> surfaces -> <repo>.
EXAMPLES_DIR = (
    pathlib.Path(__file__).resolve().parents[4] / "docs" / "api" / "python" / "examples"
)
# A mis-pointed directory must fail at collection, never silently empty the
# parametrize list (which would drop every documented example from CI).
assert EXAMPLES_DIR.is_dir(), f"documented examples directory missing: {EXAMPLES_DIR}"
assert list(EXAMPLES_DIR.glob("*.py")), f"no documented examples under {EXAMPLES_DIR}"

# The examples that need an optional third-party package (not in the test
# extra), so they cannot run in CI: named once here, with the reason.
OPTIONAL_DEP_EXAMPLES = {"visualization.py"}  # needs `neo4j-viz`


class TestDocumentedExample:
    @pytest.mark.parametrize(
        "example",
        sorted(
            path.name
            for path in EXAMPLES_DIR.glob("*.py")
            if path.name not in OPTIONAL_DEP_EXAMPLES
        ),
    )
    def test_example_runs(self, example):
        """docs/api/python/examples/<file> runs end to end."""
        import runpy
        runpy.run_path(str(EXAMPLES_DIR / example), run_name="__main__")


