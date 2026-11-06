import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
POM_PY = ROOT / "scripts" / "lib" / "pom.py"

# A pom shaped like the real one: default xmlns, comments that must
# survive a set byte-for-byte, and a dependency whose <version> must
# never be the rewrite target.
POM = """<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.relate</groupId>
  <artifactId>relate</artifactId>
  <version>0.1.0</version>
  <name>relate</name>
  <!-- a comment that ElementTree serialization would drop -->
  <dependencies>
    <dependency>
      <groupId>net.java.dev.jna</groupId>
      <artifactId>jna</artifactId>
      <version>5.14.0</version>
    </dependency>
  </dependencies>
</project>
"""


def run(*args, pom_text=POM):
    with tempfile.NamedTemporaryFile("w", suffix=".xml", delete=False) as fh:
        fh.write(pom_text)
        path = fh.name
    try:
        return subprocess.run(
            ["python3", str(POM_PY), *args, path],
            capture_output=True,
            text=True,
        )
    finally:
        Path(path).unlink()


class GetTests(unittest.TestCase):
    def test_get_prints_the_project_version(self) -> None:
        self.assertEqual(run("get").stdout.strip(), "0.1.0")

    def test_get_fails_loudly_without_a_project_version(self) -> None:
        result = run("get", pom_text="<project><name>x</name></project>\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("no project <version>", result.stderr)

    def test_get_fails_loudly_on_malformed_xml(self) -> None:
        result = run("get", pom_text="not xml at all\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("not well-formed", result.stderr)


class SetTests(unittest.TestCase):
    def test_set_rewrites_exactly_the_project_version_line(self) -> None:
        with tempfile.NamedTemporaryFile("w", suffix=".xml", delete=False) as fh:
            fh.write(POM)
            path = fh.name
        try:
            result = subprocess.run(
                ["python3", str(POM_PY), "set", path, "0.16.2"],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            lines = Path(path).read_text().split("\n")
        finally:
            Path(path).unlink()
        expected = POM.replace(
            "  <version>0.1.0</version>", "  <version>0.16.2</version>"
        ).split("\n")
        self.assertEqual(lines, expected)  # comments and the dep version intact

    def test_set_without_a_version_argument_is_a_usage_error(self) -> None:
        result = run("set", pom_text="<project><name>x</name></project>\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("usage:", result.stderr)


if __name__ == "__main__":
    unittest.main()
