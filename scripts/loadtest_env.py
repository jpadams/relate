"""The ADR-0071 loadtest environment-class model, shared by the loadtest
scripts.

The class is the Docker recipe plus runner label, not hostname or CPU
model. Report `schema_version` stays 6; `environment` is sidecar metadata.

`compare-loadtest.py` and `baseline-fingerprint.py` are hyphenated CLI
scripts Python cannot import as modules, so both load this underscore
sibling with `importlib.util.spec_from_file_location` (the established
pattern for hyphenated scripts under `scripts/`). The model lives here,
exactly once; no consumer keeps a copy. The runner-label policy lives
here too: `runner_label()` resolves BASELINE_RUNNER through the label
vocabulary and falls back on the GITHUB_ACTIONS/local default.
"""

from __future__ import annotations

import os
import sys
from typing import Any, NamedTuple

CLASS = "relate-loadtest-baseline/v1"
# Field order is the compare key. Checkpoint reports may omit ops/warmup/workers.
CLASS_FIELDS = (
    "class",
    "arch",
    "os",
    "build_image",
    "run_image",
    "cpus",
    "cpu_period_us",
    "cpu_quota_us",
    "memory",
    "ops",
    "warmup",
    "workers",
    "runner",
)


def environment_from(
    *,
    arch: str,
    os_name: str = "linux",
    build_image: str,
    run_image: str,
    cpus: int,
    cpu_period_us: int,
    cpu_quota_us: int,
    memory: str,
    ops: int | None,
    warmup: int | None,
    workers: int | None,
    runner: str,
) -> dict[str, Any]:
    return {
        "class": CLASS,
        "arch": arch,
        "os": os_name,
        "build_image": build_image,
        "run_image": run_image,
        "cpus": cpus,
        "cpu_period_us": cpu_period_us,
        "cpu_quota_us": cpu_quota_us,
        "memory": memory,
        "ops": ops,
        "warmup": warmup,
        "workers": workers,
        "runner": runner,
    }


class EnvMismatch(NamedTuple):
    """One differing environment field, from the single CLASS_FIELDS walk.

    `field` is None when either side has no environment object at all; the
    two projections (`name`, `message`) are what every consumer formats
    from, so no second diff implementation can drift from the walk.
    """

    field: str | None
    left: Any
    right: Any

    @property
    def name(self) -> str:
        return "missing environment" if self.field is None else self.field

    @property
    def message(self) -> str:
        if self.field is None:
            return "missing environment"
        return f"{self.field}: {self.left!r} != {self.right!r}"


def environment_mismatches(left: Any, right: Any) -> list[EnvMismatch]:
    """The one structured walk of the environment comparison fields."""
    if not isinstance(left, dict) or not isinstance(right, dict):
        return [EnvMismatch(None, left, right)]
    return [
        EnvMismatch(f, left.get(f), right.get(f))
        for f in CLASS_FIELDS
        if left.get(f) != right.get(f)
    ]


def in_github_actions() -> bool:
    """True inside GitHub Actions — the class-mismatch hard-fail context.

    The one CI detection every environment-class compare shares: a
    mismatch hard-fails in Actions and a local `make gate` run reports
    it and moves on (ADR-0071). Both compare scripts call this and the
    runner-label default resolves through it; no consumer keeps its
    own copy.
    """
    return os.environ.get("GITHUB_ACTIONS") == "true"


def _valid_runner_label(label: str) -> bool:
    """The one runner-label vocabulary check.

    A valid label is `local` or `github-actions:<class>` with a non-empty
    class — anything else is invalid, the literal `BASELINE_RUNNER` the
    .sonic `KEY: KEY` env forwarding passes when the host variable is unset
    included. `runner_label()` is the only caller; nothing else re-derives
    the vocabulary.
    """
    if label == "local":
        return True
    if not label.startswith("github-actions:"):
        return False
    return label != "github-actions:"


def runner_label() -> str:
    """The runner label: BASELINE_RUNNER when it holds a valid label, else
    the GITHUB_ACTIONS/local default. An invalid explicit value — the
    forwarded literal, or an empty value that is set rather than unset —
    is named in one stderr note and ignored, never stamped.
    """
    explicit = os.environ.get("BASELINE_RUNNER")
    if explicit is not None:
        if _valid_runner_label(explicit):
            return explicit
        print(
            f"loadtest_env: ignoring invalid BASELINE_RUNNER {explicit!r} "
            "(expected 'local' or 'github-actions:<class>'); "
            "falling back to the GITHUB_ACTIONS/local default",
            file=sys.stderr,
        )
    if in_github_actions():
        return "github-actions:ubuntu-24.04"
    return "local"
