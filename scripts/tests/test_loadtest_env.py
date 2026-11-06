import unittest

from script_loader import ROOT, load

# The shared environment-class model (scripts/loadtest_env.py): its model
# and runner-label policy tests live here, beside the module they cover —
# not in a consumer CLI's suite.
env_mod = load("loadtest_env", ROOT / "scripts" / "loadtest_env.py")

# The seven shared environment kwargs — the Docker-recipe half of the
# class (images, cpus, cpu period/quota, memory). Spelled exactly once;
# each model test spreads this and contributes only the fields its
# shape differs on (ops/warmup/workers/runner).
BASE_ENV_KWARGS = {
    "arch": "x86_64",
    "build_image": "rust:1.88-bookworm@sha256:aaa",
    "run_image": "debian:bookworm-slim@sha256:bbb",
    "cpus": 2,
    "cpu_period_us": 10000,
    "cpu_quota_us": 20000,
    "memory": "2g",
}


class LoadtestEnvModelTests(unittest.TestCase):
    """The one structured walk every environment comparison formats from."""

    def test_mismatches_report_missing_and_field_mismatch(self) -> None:
        mismatches = env_mod.environment_mismatches(None, {})
        self.assertEqual([m.message for m in mismatches], ["missing environment"])
        left = env_mod.environment_from(
            **BASE_ENV_KWARGS,
            ops=20000,
            warmup=8,
            workers=8,
            runner="github-actions:ubuntu-24.04",
        )
        right = dict(left, runner="local")
        mismatches = env_mod.environment_mismatches(left, right)
        self.assertEqual(
            [m.message for m in mismatches],
            ["runner: 'github-actions:ubuntu-24.04' != 'local'"],
        )
        self.assertEqual([m.name for m in mismatches], ["runner"])
        self.assertEqual(env_mod.environment_mismatches(left, left), [])

    def test_environment_from_stamps_the_shared_class(self) -> None:
        env = env_mod.environment_from(
            **BASE_ENV_KWARGS,
            ops=None,
            warmup=None,
            workers=None,
            runner="local",
        )
        self.assertEqual(env["class"], env_mod.CLASS)
        self.assertEqual(tuple(env), env_mod.CLASS_FIELDS)


class RunnerLabelTests(unittest.TestCase):
    """loadtest_env's runner_label() resolves BASELINE_RUNNER through the
    label vocabulary.

    An invalid explicit value — an out-of-vocabulary label (`staging-box`),
    the empty class (`github-actions:`), the literal `BASELINE_RUNNER` the
    .sonic `KEY: KEY` env forwarding passes when the host variable is
    unset, or an empty value that is set rather than unset — gets one
    stderr note naming it, never a stamp, and the GITHUB_ACTIONS/local
    default applies.
    """

    INVALID_LABELS = ("staging-box", "github-actions:", "BASELINE_RUNNER", "")

    def _runner_label(self, env: dict[str, str]) -> tuple[str, str]:
        """Call env_mod.runner_label() under env; return the label and stderr."""
        import contextlib
        import io
        import os
        from unittest import mock

        err = io.StringIO()
        with mock.patch.dict(
            os.environ, env, clear=True
        ), contextlib.redirect_stderr(err):
            return env_mod.runner_label(), err.getvalue()

    def test_valid_explicit_label_passes_through(self) -> None:
        label, err = self._runner_label(
            {"BASELINE_RUNNER": "github-actions:linux-4-core-x86"}
        )
        self.assertEqual(label, "github-actions:linux-4-core-x86")
        self.assertEqual(err, "")
        label, _ = self._runner_label({"BASELINE_RUNNER": "local"})
        self.assertEqual(label, "local")

    def test_invalid_explicit_label_falls_back_to_the_actions_default(self) -> None:
        for value in self.INVALID_LABELS:
            with self.subTest(value=value):
                label, err = self._runner_label(
                    {"BASELINE_RUNNER": value, "GITHUB_ACTIONS": "true"}
                )
                self.assertEqual(label, "github-actions:ubuntu-24.04")
                self.assertIn(f"ignoring invalid BASELINE_RUNNER {value!r}", err)

    def test_invalid_explicit_label_falls_back_to_local(self) -> None:
        for value in self.INVALID_LABELS:
            with self.subTest(value=value):
                label, err = self._runner_label({"BASELINE_RUNNER": value})
                self.assertEqual(label, "local")
                self.assertIn(f"ignoring invalid BASELINE_RUNNER {value!r}", err)


if __name__ == "__main__":
    unittest.main()
