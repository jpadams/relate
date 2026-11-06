#!/usr/bin/env python3
"""stellar-review per-pass model config (ADR-0302).

Turns the `STELLAR_MODEL_<PASS>` env overrides into OpenCode V2 agent
config: for every pass in briefs.PASS_LAYOUT whose env value is
non-empty, sets `agents["stellar-<pass>"].model` (verbatim) in the target
OpenCode config, merged into its existing JSON with every other key kept.
The repo's .opencode/agents/stellar-*.md files set no `model`, so the
value survives OpenCode's config merge and pins that pass's subagent.

With no override set the target file is neither created nor modified.
An existing target that is not a JSON object, or whose `agents` (or an
`agents["stellar-<pass>"]` entry) is not an object, fails the run and is
never overwritten.

Usage:
    pass_models.py [--config PATH]
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import shutil
import sys
import tempfile
from collections.abc import Mapping
from typing import NoReturn

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import briefs  # noqa: E402
from fileout import ensure_parent  # noqa: E402

DEFAULT_CONFIG = "~/.config/opencode/opencode.json"


def pass_model_env() -> dict[str, str]:
    """{pass name: `STELLAR_MODEL_<PASS>`} for every pass in
    briefs.PASS_LAYOUT, read from the registry at call time. The one home
    of the pass -> env var rule; the tests (the workflow and SKILL.md
    drift guard included) read the names from here."""
    return {name: "STELLAR_MODEL_" + name.upper() for name in briefs.PASS_LAYOUT}


def agent_id(name: str) -> str:
    """The OpenCode agent a pass runs as (.opencode/agents/<id>.md)."""
    return "stellar-" + name


def overrides(environ: Mapping[str, str]) -> dict[str, str]:
    """{pass name: model} for each registered pass whose
    `STELLAR_MODEL_<PASS>` value is non-empty, in registry order."""
    return {name: environ[var] for name, var in pass_model_env().items() if environ.get(var)}


def refuse(path: str, what: str) -> NoReturn:
    """Fails the run without writing the target config at `path`."""
    raise SystemExit(f"pass_models: {path}: {what}; refusing to overwrite")


def merged_config(path: str, applied: Mapping[str, str]) -> dict:
    """The target's JSON object ({} when the file does not exist yet) with
    `agents.<id>.model` set for each {pass name: model} override. Exits
    without touching the file unless it is a JSON object whose `agents`
    and the `agents.<id>` of each overridden pass, where present, are
    objects."""
    try:
        with open(path) as fh:
            doc = json.load(fh)
    except FileNotFoundError:
        doc = {}
    except json.JSONDecodeError as e:
        refuse(path, f"not valid JSON ({e})")
    if not isinstance(doc, dict):
        refuse(path, "top level is not a JSON object")
    agents = doc.setdefault("agents", {})
    if not isinstance(agents, dict):
        refuse(path, "`agents` is not an object")
    for name, model in applied.items():
        agent = agent_id(name)
        entry = agents.setdefault(agent, {})
        if not isinstance(entry, dict):
            refuse(path, f"`agents.{agent}` is not an object")
        entry["model"] = model
    return doc


def write_atomic(path: str, doc: dict) -> None:
    """Temp file in the target's directory, then os.replace — a reader
    never sees a half-written config. Keeps an existing file's mode."""
    parent = os.path.dirname(ensure_parent(path))
    fd, tmp = tempfile.mkstemp(dir=parent, prefix=".pass_models-", suffix=".json")
    try:
        with os.fdopen(fd, "w") as fh:
            fh.write(json.dumps(doc, indent=2) + "\n")
        with contextlib.suppress(FileNotFoundError):
            shutil.copymode(path, tmp)
        os.replace(tmp, path)
    except BaseException:
        os.remove(tmp)
        raise


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--config", default=DEFAULT_CONFIG,
                    help=f"OpenCode config file to merge into (default: {DEFAULT_CONFIG})")
    args = ap.parse_args()
    # resolve symlinks so a linked dotfile is updated in place, not replaced
    path = os.path.realpath(os.path.expanduser(args.config))
    applied = overrides(os.environ)
    if not applied:
        print(f"pass_models: no STELLAR_MODEL_* override set; {path} left untouched")
        return
    write_atomic(path, merged_config(path, applied))
    for name in applied:
        print(f"pass_models: {name} -> agents.{agent_id(name)}.model ({path})")


if __name__ == "__main__":
    main()
