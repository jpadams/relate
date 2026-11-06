"""pass_models.py turns the STELLAR_MODEL_<PASS> env into OpenCode
`agents.stellar-<pass>.model` config. Every run targets a temp `--config`
with HOME pointed into the temp dir — never the real ~/.config."""

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from helpers import PASSES, ROOT, split_frontmatter  # also puts tools/ on sys.path

import briefs
import pass_models

SCRIPT = pass_models.__file__


class TestPassModels(unittest.TestCase):
    def setUp(self):
        # realpath: the script names the resolved file (macOS /var → /private/var)
        self.dir = os.path.realpath(tempfile.mkdtemp())
        self.cfg = os.path.join(self.dir, "opencode", "opencode.json")

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def run_tool(self, models):
        pass_env = pass_models.pass_model_env()
        env = {k: v for k, v in os.environ.items() if k not in pass_env.values()}
        env["HOME"] = self.dir
        env.update({pass_env[name]: model for name, model in models.items()})
        return subprocess.run(
            [sys.executable, SCRIPT, "--config", self.cfg],
            env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False)

    def write_cfg(self, text):
        os.makedirs(os.path.dirname(self.cfg), exist_ok=True)
        with open(self.cfg, "w") as fh:
            fh.write(text)

    def read_cfg(self):
        with open(self.cfg) as fh:
            return json.load(fh)

    def test_none_set_writes_no_file(self):
        for models in ({}, {name: "" for name in PASSES}):
            with self.subTest(models=models):
                proc = self.run_tool(models)
                self.assertEqual(proc.returncode, 0, proc.stderr)
                self.assertFalse(os.path.exists(os.path.dirname(self.cfg)))
                lines = proc.stdout.splitlines()
                self.assertEqual(len(lines), 1, proc.stdout)
                self.assertIn("no STELLAR_MODEL_* override set", lines[0])

    def test_none_set_leaves_existing_file_untouched(self):
        self.write_cfg("not json, and not ours to touch")
        proc = self.run_tool({name: "" for name in PASSES})
        self.assertEqual(proc.returncode, 0, proc.stderr)
        with open(self.cfg) as fh:
            self.assertEqual(fh.read(), "not json, and not ours to touch")

    def test_two_set_produce_exactly_those_agents(self):
        first, last = PASSES[0], PASSES[-1]
        models = {name: "" for name in PASSES}
        models.update({first: "prov/strong#high", last: "prov/cheap"})
        proc = self.run_tool(models)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(self.read_cfg(), {"agents": {
            f"stellar-{first}": {"model": "prov/strong#high"},
            f"stellar-{last}": {"model": "prov/cheap"},
        }})
        lines = proc.stdout.splitlines()
        self.assertEqual(len(lines), 2, proc.stdout)
        for line, name in zip(lines, (first, last)):
            self.assertIn(name, line)
            self.assertIn(f"stellar-{name}", line)
        self.assertEqual(os.listdir(os.path.dirname(self.cfg)), ["opencode.json"],
                         "atomic write left a temp file behind")

    def test_merge_preserves_existing_keys_and_agents(self):
        name = PASSES[0]
        existing = {
            "$schema": "https://opencode.ai/config.json",
            "provider": {"p": {"options": {"timeout": 5}}},
            "agents": {
                "build": {"model": "prov/build"},
                f"stellar-{name}": {"description": "kept", "model": "prov/old"},
            },
        }
        self.write_cfg(json.dumps(existing))
        os.chmod(self.cfg, 0o644)
        proc = self.run_tool({name: "prov/new"})
        self.assertEqual(proc.returncode, 0, proc.stderr)
        expected = json.loads(json.dumps(existing))
        expected["agents"][f"stellar-{name}"]["model"] = "prov/new"
        self.assertEqual(self.read_cfg(), expected)
        self.assertEqual(stat.S_IMODE(os.stat(self.cfg).st_mode), 0o644)

    def test_symlinked_config_is_updated_through_the_link(self):
        real = os.path.join(self.dir, "dotfiles", "opencode.json")
        os.makedirs(os.path.dirname(real))
        with open(real, "w") as fh:
            fh.write("{}")
        os.makedirs(os.path.dirname(self.cfg))
        os.symlink(real, self.cfg)
        proc = self.run_tool({PASSES[0]: "prov/m"})
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertTrue(os.path.islink(self.cfg))
        self.assertEqual(self.read_cfg(), {"agents": {f"stellar-{PASSES[0]}": {"model": "prov/m"}}})

    def test_malformed_existing_config_exits_nonzero_and_is_kept(self):
        cases = {
            "invalid JSON": "{not json",
            "top level not an object": "[]",
            "agents not an object": json.dumps({"agents": ["stellar-x"]}),
            "agent entry not an object": json.dumps(
                {"agents": {f"stellar-{PASSES[0]}": "prov/m"}}),
        }
        for label, text in cases.items():
            with self.subTest(case=label):
                self.write_cfg(text)
                proc = self.run_tool({PASSES[0]: "prov/m"})
                self.assertNotEqual(proc.returncode, 0)
                self.assertIn(self.cfg, proc.stderr)
                with open(self.cfg) as fh:
                    self.assertEqual(fh.read(), text, "malformed config was overwritten")

    def test_every_agent_id_has_a_model_free_opencode_agent_file(self):
        proc = self.run_tool({name: "prov/m" for name in PASSES})
        self.assertEqual(proc.returncode, 0, proc.stderr)
        for agent in self.read_cfg()["agents"]:
            with self.subTest(agent=agent):
                path = os.path.join(ROOT, ".opencode", "agents", f"{agent}.md")
                self.assertTrue(os.path.exists(path), path)
                with open(path) as fh:
                    fm, _ = split_frontmatter(fh.read())
                # a project-level model scalar would replace the global one
                self.assertNotRegex(fm, r"(?m)^model:", path)

    def test_iterates_the_pass_registry(self):
        proc = self.run_tool({name: f"prov/{name}" for name in PASSES})
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(list(self.read_cfg()["agents"]), [f"stellar-{n}" for n in PASSES])
        # a pass added to the registry is picked up with no second list to
        # edit: against the grown registry the module reads an env var for
        # it and overrides its agent
        absent = os.path.join(self.dir, "absent.json")
        with mock.patch.dict(briefs.PASS_LAYOUT, {"extra": {"chunked": False, "sections": []}}):
            pass_env = pass_models.pass_model_env()
            self.assertEqual(list(pass_env), [*PASSES, "extra"])
            got = pass_models.merged_config(
                absent, pass_models.overrides({pass_env["extra"]: "prov/x"}))
        self.assertEqual(got, {"agents": {"stellar-extra": {"model": "prov/x"}}})
        self.assertEqual(list(pass_models.pass_model_env()), list(PASSES))


if __name__ == "__main__":
    unittest.main()
