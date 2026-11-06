"""The stellar pass agents ship as two harness copies (.opencode/agents/
for CI, .claude/agents/ for local Claude). Their bodies must stay in sync;
only the frontmatter differs."""

import os
import unittest

from helpers import PASSES, ROOT, split_frontmatter


class TestAgentSync(unittest.TestCase):
    def test_bodies_identical(self):
        for name in PASSES:
            oc_path = os.path.join(ROOT, ".opencode", "agents", f"stellar-{name}.md")
            cc_path = os.path.join(ROOT, ".claude", "agents", f"stellar-{name}.md")
            with open(oc_path) as fh:
                _, oc_body = split_frontmatter(fh.read())
            with open(cc_path) as fh:
                _, cc_body = split_frontmatter(fh.read())
            self.assertEqual(oc_body, cc_body, f"stellar-{name}: harness copies drifted")

    def test_opencode_frontmatter_is_subagent_readonly(self):
        for name in PASSES:
            oc_path = os.path.join(ROOT, ".opencode", "agents", f"stellar-{name}.md")
            with open(oc_path) as fh:
                fm, _ = split_frontmatter(fh.read())
            self.assertIn("mode: subagent", fm, name)
            self.assertIn("edit", fm, name)
            self.assertIn("deny", fm, name)

    def test_mandate_files_exist(self):
        for name in PASSES:
            p = os.path.join(ROOT, ".agents", "skills", "stellar-review", "passes", f"{name}.md")
            self.assertTrue(os.path.exists(p), p)
            # and every pass file the agent bodies reference exists
        refs = ("full-checklist.md", "finding-schema.md", "severity-rubric.md")
        for r in refs:
            p = os.path.join(ROOT, ".agents", "skills", "stellar-review", "references", r)
            self.assertTrue(os.path.exists(p), p)


if __name__ == "__main__":
    unittest.main()
