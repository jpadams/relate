"""Drift guard for the review-model env vocabulary (ADR-0302): the four
`STELLAR_MODEL_<PASS>` names are the ones the generator reads
(`pass_models.pass_model_env()`, derived from the canonical production pass
registry `briefs.PASS_LAYOUT`), and every copy of the vocabulary — the
reusable workflow's pass-model step env and workflow_call secrets, its
callers' secrets blocks (every watched `opencode-review-*` workflow but the
reusable one), SKILL.md's MODEL TIERS rule, and the
review-scripts-test path filter — must agree with it. The pass names stay
out of the shared Run OpenCode step, whose only override is
OPENCODE_REVIEW_MODEL. The default model pin the override falls back to
must match ADR-0302's Decision table in both workflows that carry it."""

import glob
import os
import re
import unittest

from helpers import ROOT, read, region_between  # also puts tools/ on sys.path

import pass_models

WORKFLOWS = os.path.join(ROOT, ".github", "workflows")
REUSABLE = os.path.join(WORKFLOWS, "opencode-review-reusable.yml")
NIGHTSHIFT = os.path.join(WORKFLOWS, "stellar-nightshift.yml")
REVIEW_SCRIPTS_TEST = os.path.join(WORKFLOWS, "review-scripts-test.yml")
SKILL = os.path.join(ROOT, ".agents", "skills", "stellar-review", "SKILL.md")
ADR_0302 = os.path.join(ROOT, "docs", "adr", "0302-review-model-env-override.md")

# Never re-typed: the four pass-tier names are exactly the ones the
# generator reads (pass_models.pass_model_env()), plus the run-level
# override every review shares.
PASS_TIER_NAMES = tuple(pass_models.pass_model_env().values())
SECRET_NAMES = ("OPENCODE_REVIEW_MODEL",) + PASS_TIER_NAMES

# A whole STELLAR_MODEL_* token: `\w+` runs to the end of the identifier, so
# a renamed name (`STELLAR_MODEL_STRUCTURE_`) is never truncated back into a
# registered one.
MODEL_TOKEN = re.compile(r"\bSTELLAR_MODEL_\w+")

# Each review run and the env var its `opencode run --model` reads; the
# var's env line carries the default pin.
MODEL_RUNS = (
    (REUSABLE, "OPENCODE_REVIEW_MODEL"),
    (NIGHTSHIFT, "STELLAR_NIGHTSHIFT_MODEL"),
)

# review-scripts-test.yml must run when any of these change, or this guard
# never executes in CI. REVIEW_GLOB is also the caller set's source.
REVIEW_GLOB = ".github/workflows/opencode-review-*.yml"
WATCHED_PATHS = (
    REVIEW_GLOB,
    ".github/workflows/stellar-nightshift.yml",
    ".agents/skills/stellar-review/SKILL.md",
)

# The reusable workflow's callers: every watched review workflow but the
# reusable one itself, so a new caller is guarded without being listed.
CALLERS = tuple(
    sorted(
        path
        for path in glob.glob(os.path.join(ROOT, REVIEW_GLOB))
        if path != REUSABLE
    )
)


def mapping_entry(region, name):
    """The lines of the `name:` entry inside a YAML mapping region, up to
    the next key at the same indentation (or the end of the region)."""
    m = re.search(r"^( *)" + re.escape(name) + r": *$", region, re.M)
    if m is None:
        return None
    rest = region[m.end():]
    nxt = re.search(r"^ {" + str(len(m.group(1))) + r"}\S", rest, re.M)
    return rest[: nxt.start()] if nxt else rest


def env_default(text, name):
    """The `|| '<literal>'` fallback on `name`'s env line."""
    m = re.search(r"(?m)^ *" + re.escape(name) + r":.*\|\| *'([^']+)'", text)
    if m is None:
        raise AssertionError(f"{name}: no `|| '<default>'` fallback on its env line")
    return m.group(1)


def env_value(step, name):
    """The value on `name`'s env line inside `step`, or None when absent."""
    m = re.search(r"^ *" + re.escape(name) + r": *(.*)$", step, re.M)
    return m.group(1) if m else None


def secrets_block(path):
    """`path`'s text after its one `secrets:` line; fails unless the file
    has exactly one."""
    text = read(path)
    starts = list(re.finditer(r"^ *secrets: *$", text, re.M))
    if len(starts) != 1:
        raise AssertionError(
            f"{path}: expected exactly one `secrets:` block, found {len(starts)}"
        )
    return text[starts[0].end():]


def model_step():
    """The reusable workflow's Run OpenCode step (the shared model run)."""
    return region_between(
        read(REUSABLE), r"^ *- name: Run OpenCode", r"^ *- name: ", REUSABLE
    )


def pass_model_step():
    """The reusable workflow's stellar-only step that turns the pass
    overrides into OpenCode agent config (pass_models.py)."""
    return region_between(
        read(REUSABLE),
        r"^ *- name: Apply stellar pass-model overrides",
        r"^ *- name: ",
        REUSABLE,
    )


def model_tiers_rule():
    """SKILL.md's MODEL TIERS rule, through the next `## ` heading."""
    return region_between(read(SKILL), r"^#* *MODEL TIERS", r"^## ", SKILL)


class TestModelEnvDrift(unittest.TestCase):
    def assert_forwarded(self, step, label, name):
        value = env_value(step, name)
        self.assertIsNotNone(value, f"{REUSABLE}: {name} missing from the {label} env")
        self.assertIn(
            f"secrets.{name}",
            value,
            f"{REUSABLE}: {name} in the {label} env must read the secret",
        )
        self.assertIn(
            f"vars.{name}",
            value,
            f"{REUSABLE}: {name} in the {label} env must fall back to the repo variable",
        )

    def test_model_step_env_forwards_only_the_run_override(self):
        step = model_step()
        self.assert_forwarded(step, "Run OpenCode step", "OPENCODE_REVIEW_MODEL")
        self.assertEqual(
            MODEL_TOKEN.findall(step),
            [],
            f"{REUSABLE}: the Run OpenCode step must carry no STELLAR_MODEL_* "
            "name — pass overrides apply through the pass-model step's "
            "OpenCode config, never the shared model step env",
        )

    def test_pass_model_step_env_forwards_every_pass(self):
        step = pass_model_step()
        for name in PASS_TIER_NAMES:
            with self.subTest(name=name):
                self.assert_forwarded(step, "pass-model step", name)

    def test_pass_model_step_configures_the_scripted_run(self):
        step = pass_model_step()
        self.assertRegex(
            step,
            r"(?m)^ *if: inputs\.pipeline == 'scripted' *$",
            f"{REUSABLE}: the pass-model step must run only for the scripted pipeline",
        )
        self.assertRegex(
            step,
            r"(?m)^ *run: python3 \.agents/skills/stellar-review/tools/pass_models\.py *$",
            f"{REUSABLE}: the pass-model step must run pass_models.py",
        )
        text = read(REUSABLE)
        self.assertLess(
            text.index(step),
            text.index(model_step()),
            f"{REUSABLE}: the pass-model step must run before the Run OpenCode step",
        )

    def test_no_unregistered_model_token(self):
        # Negative assertion: every STELLAR_MODEL_* token present must be a
        # registered pass tier — a renamed or an extra name fails.
        for label, region in ((REUSABLE, pass_model_step()), (SKILL, model_tiers_rule())):
            with self.subTest(surface=label):
                found = set(MODEL_TOKEN.findall(region))
                self.assertEqual(
                    found,
                    set(PASS_TIER_NAMES),
                    f"{label}: STELLAR_MODEL_* tokens drifted from "
                    f"pass_models.pass_model_env() (extra: "
                    f"{sorted(found - set(PASS_TIER_NAMES))}, missing: "
                    f"{sorted(set(PASS_TIER_NAMES) - found)})",
                )

    def test_default_pin_matches_adr_0302(self):
        pins = {var: env_default(read(path), var) for path, var in MODEL_RUNS}
        distinct = set(pins.values())
        self.assertEqual(
            len(distinct),
            1,
            f"the {' and '.join(pins)} default pins drifted apart: {pins}",
        )
        (pin,) = distinct
        decision = region_between(read(ADR_0302), r"^## Decision", r"^## ", ADR_0302)
        self.assertIn(
            pin,
            decision,
            f"{ADR_0302}: default {pin!r} is not in the Decision table — "
            "bumping the default requires a new ADR (accepted ADRs are "
            "immutable)",
        )

    def test_reusable_declares_the_workflow_call_secrets(self):
        rest = secrets_block(REUSABLE)
        jobs = re.search(r"^jobs:", rest, re.M)
        self.assertIsNotNone(
            jobs, f"{REUSABLE}: no `jobs:` after the workflow_call secrets map"
        )
        declared = rest[: jobs.start()]
        for name in SECRET_NAMES:
            with self.subTest(name=name):
                entry = mapping_entry(declared, name)
                self.assertIsNotNone(
                    entry,
                    f"{REUSABLE}: {name} not declared under on.workflow_call.secrets",
                )
                self.assertRegex(
                    entry,
                    r"(?m)^ *required: false *$",
                    f"{REUSABLE}: {name} must be declared required: false",
                )
        entry = mapping_entry(declared, "FIREWORKS_API_KEY")
        self.assertIsNotNone(
            entry, f"{REUSABLE}: FIREWORKS_API_KEY not declared under on.workflow_call.secrets"
        )
        self.assertRegex(
            entry,
            r"(?m)^ *required: true *$",
            f"{REUSABLE}: FIREWORKS_API_KEY must be declared required: true",
        )

    def test_callers_call_the_reusable_workflow(self):
        # A REVIEW_GLOB match that is not a caller must fail here, not be
        # held to (or silently skip) the secrets contract below.
        self.assertTrue(CALLERS, f"no caller workflow matches {REVIEW_GLOB}")
        uses = "./" + os.path.relpath(REUSABLE, ROOT)
        for path in CALLERS:
            with self.subTest(caller=os.path.basename(path)):
                self.assertRegex(
                    read(path),
                    r"(?m)^ *uses: *" + re.escape(uses) + r" *$",
                    f"{path}: matches {REVIEW_GLOB} but has no `uses: {uses}`",
                )

    def test_callers_pass_every_secret(self):
        for path in CALLERS:
            block = secrets_block(path)
            for name in SECRET_NAMES:
                with self.subTest(caller=os.path.basename(path), name=name):
                    pat = (
                        r"(?m)^ *"
                        + re.escape(name)
                        + r": *"
                        + re.escape("${{ secrets." + name + " }}")
                        + r" *$"
                    )
                    self.assertRegex(
                        block,
                        pat,
                        f"{path}: {name} is not passed as ${{{{ secrets.{name} }}}}",
                    )

    def test_review_runs_use_variable_models(self):
        for path, var in MODEL_RUNS:
            with self.subTest(workflow=os.path.basename(path)):
                tokens = re.findall(r"opencode run --model +(\S+)", read(path))
                self.assertTrue(tokens, f"{path}: no `opencode run --model` invocation")
                for token in tokens:
                    self.assertEqual(
                        token,
                        f'"${var}"',
                        f'{path}: `opencode run --model` must read "${var}"',
                    )

    def test_review_scripts_test_watches_the_guarded_files(self):
        paths = region_between(
            read(REVIEW_SCRIPTS_TEST),
            r"^ *pull_request:",
            r"^ *workflow_dispatch:",
            REVIEW_SCRIPTS_TEST,
        )
        for entry in WATCHED_PATHS:
            with self.subTest(path=entry):
                self.assertRegex(
                    paths,
                    r'(?m)^ *- "?' + re.escape(entry) + r'"? *$',
                    f"{REVIEW_SCRIPTS_TEST}: pull_request paths must include {entry}",
                )


if __name__ == "__main__":
    unittest.main()
