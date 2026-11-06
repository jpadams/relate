"""Shared importlib loader for the scripts/tests/ suites.

The subjects under scripts/ are hyphenated (`ci-timings-report.py`,
`baseline-fingerprint.py`), so they are not importable by name; each test
loads its subject from a path. The loader lives exactly once here (the two
suites previously carried verbatim copies) and is imported as a plain
module — scripts/tests/ is sys.path[0] when the suite runs.
"""

import importlib.util
import sys
from pathlib import Path
from types import ModuleType

ROOT = Path(__file__).resolve().parents[2]


def load(name: str, path: Path) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    # Register before exec — the documented way to run a module from a
    # path. Classes resolve their own module through sys.modules:
    # dataclasses, typing.get_type_hints, and pickling all look there, and
    # without the registration a dataclass in the loaded module crashes
    # on Python 3.12 (its _is_type probe resolves *string* annotations
    # through sys.modules[cls.__module__], which was None).
    prior = sys.modules.get(name)
    sys.modules[name] = mod
    try:
        spec.loader.exec_module(mod)
    except BaseException:
        # Remove exactly what this call installed: restore a prior
        # entry, delete only when there was none.
        if prior is None:
            del sys.modules[name]
        else:
            sys.modules[name] = prior
        raise
    return mod
