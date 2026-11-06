#!/usr/bin/env python3
"""Recursive key/type diff between the two releases.json fixtures.

Checks the schema-contract acceptance of the releases-json-discovery
feature: the release-workflow generator's committed expected output
(`.github/scripts/tests/fixtures/publish-releases-json-expected.json`) and
the CLI's parser fixture
(`surfaces/relate-cli/tests/fixtures/releases.json`) must describe the
same document skeleton:

  * the same keys at every level,
  * the same value types at every key path, with `null` a distinct type,
  * array indices collapsed to `[]`, so arrays are compared by their
    elements' type signature, never by length or index,
  * leaf values ignored (only their types matter).

It is the structural equivalent of `jq -S '[paths(scalars)]'` with the
array-length and leaf-value noise removed. Empty stdout means the skeletons
are identical; the exit status is 0 iff the diff is empty.

Usage: compare-releases-json-skeletons.py [GENERATOR_FIXTURE CLI_FIXTURE]
"""

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
GENERATOR_FIXTURE = (
    REPO_ROOT
    / ".github/scripts/tests/fixtures/publish-releases-json-expected.json"
)
CLI_FIXTURE = REPO_ROOT / "surfaces/relate-cli/tests/fixtures/releases.json"


def type_name(value):
    """JSON type of a value; `null` is a type of its own."""
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, (int, float)):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    raise AssertionError(f"unhandled JSON value: {value!r}")


def walk(value, path, out):
    """Collect, per index-collapsed path, the set of value types seen."""
    out.setdefault(path, set()).add(type_name(value))
    if isinstance(value, dict):
        for key, child in value.items():
            walk(child, path + (key,), out)
    elif isinstance(value, list):
        for item in value:
            walk(item, path + ("[]",), out)


def render(path):
    return "$" + "".join(f".{seg}" if seg != "[]" else "[]" for seg in path)


def show(types):
    return "missing" if types is None else "|".join(sorted(types))


def diff(paths_a, paths_b):
    lines = []
    for path in sorted(set(paths_a) | set(paths_b)):
        types_a = paths_a.get(path)
        types_b = paths_b.get(path)
        if types_a == types_b:
            continue
        lines.append(f"{render(path)}: generator={show(types_a)} cli={show(types_b)}")
    return lines


def main(argv):
    path_a, path_b = GENERATOR_FIXTURE, CLI_FIXTURE
    if len(argv) == 3:
        path_a, path_b = Path(argv[1]), Path(argv[2])
    elif len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    with open(path_a, encoding="utf-8") as fh:
        doc_a = json.load(fh)
    with open(path_b, encoding="utf-8") as fh:
        doc_b = json.load(fh)
    paths_a, paths_b = {}, {}
    walk(doc_a, (), paths_a)
    walk(doc_b, (), paths_b)
    for line in diff(paths_a, paths_b):
        print(line)
    return 0 if paths_a == paths_b else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
