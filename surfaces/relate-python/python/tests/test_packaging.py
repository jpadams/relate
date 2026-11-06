"""Wheel packaging assertions (PEP 561 typing, the bundled library skill)."""

from pathlib import Path

import relate


class TestLibrarySkill:
    def test_library_skill_ships_in_the_package(self):
        """The library-skills.io skill (relate/.agents/skills/relate/SKILL.md)
        is present in the installed package with Agent Skills frontmatter."""
        skill = (
            Path(relate.__file__).parent / ".agents" / "skills" / "relate" / "SKILL.md"
        )
        assert skill.is_file(), f"library skill missing: {skill}"
        text = skill.read_text()
        assert text.startswith("---")
        assert "name: relate" in text.split("---")[1]
        assert "import relate" in text

    def test_py_typed_marker_ships(self):
        """PEP 561 marker present so type checkers see the annotations."""
        marker = Path(relate.__file__).parent / "py.typed"
        assert marker.is_file(), f"py.typed missing: {marker}"
