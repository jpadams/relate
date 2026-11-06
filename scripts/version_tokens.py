#!/usr/bin/env python3
"""The one standalone version-token rule behind `scripts/release-bump.sh`.

The docs sweep rewrites every product version token in the files listed by
`docs_version_files`, and the step-4 consistency self-check re-reads the same
files to prove no stale token survived. Both call this module (through the
`rewrite` and `check` subcommands), so the rule that decides what a token is
exists exactly once — a shell character-class approximation cannot drift from
the checker that is supposed to police it.

The rule, in the order the code applies it:

1. **Scan** every maximal run of digits and dots (`[0-9.]+`) in the text. A
   run stops at anything else, so `v0.13.0` yields `0.13.0` and
   `0.13.0-rc.1` yields `0.13.0` (the `-rc.1` suffix sits outside the run and
   therefore survives a rewrite untouched).
2. **Trim trailing dots.** A dot that ends a run is punctuation (a sentence
   full stop) or a separator (a dotted filename suffix), never part of the
   version: `Released as 0.13.0.` and `relate-0.13.0.tar.gz` both carry
   the token `0.13.0`.
3. **Classify.** A run is a version token when, after that trim, `VERSION`
   matches it whole — three non-empty all-digit components. `VERSION` is the
   only spelling of that shape in this module: `is_version_token` is a
   `VERSION.fullmatch` and `--to`/`--expect` accept exactly the same shape, so
   the sweep and its self-check cannot describe different tokens. Anything
   else — a dotted quad (`0.0.0.0`), a four-part build number (`0.13.0.0`), an
   empty component (`1..2.3`), a bare number (`8080`), a dot-adjacent fragment
   (`.1` in `-rc.1`) — is not a version token: `rewrite` leaves it alone and
   `check` never counts it as stale.

Only step 3's tokens are rewritten. A run cannot swallow its neighbour's
punctuation the way a `(^|[^0-9.])…([^0-9.]|$)` ERE match did, so two tokens
on one line are both found, and a token bounded by dots on either side is
still found when those dots are trailing punctuation.

`rewrite` returns every non-token run it skipped, and its CLI reports the
version-shaped ones (three or more dotted components: a quad, a four-part
number, an empty component) because those are the near misses a maintainer
would wonder about. A bare integer is skipped without comment — a release log
that listed every number in a README would bury the answer.

Usage:

    scripts/version_tokens.py rewrite --to 0.14.0 README.md docs/...
    scripts/version_tokens.py check --expect 0.14.0 README.md docs/...

`rewrite` edits files in place and reports the non-token runs it left alone on
stderr; `check` prints any version token that is not `--expect` and exits 1.
Both visit every listed file before exiting and name each one they could not
read or write, so one bad path never hides the rest of the sweep; either exits
2 when a file was unreadable or unwritable (argparse also exits 2 on a usage
error).
"""

from __future__ import annotations

import argparse
import re
import sys
from collections.abc import Iterator
from pathlib import Path

#: How many dotted numeric components a release version has. The one place the
#: arity is written: `VERSION` joins exactly this many numeric components, and
#: `is_version_shaped` calls a run at or above it a near miss worth reporting.
COMPONENTS = 3

#: The scan unit: a maximal run of digits and dots.
RUN = re.compile(r"[0-9.]+")

#: The accepted shape of the release version both subcommands compare against,
#: and — through `is_version_token` — the one spelling of what a token is.
#: Derived from `COMPONENTS`, so the shape is not stated twice.
VERSION = re.compile(r"\.".join(["[0-9]+"] * COMPONENTS))


def is_version_token(candidate: str) -> bool:
    """True when `candidate` is exactly three non-empty numeric components."""
    return VERSION.fullmatch(candidate) is not None


def _trimmed(run: str) -> str:
    """`run` without its trailing dots — step 2's one home.

    A dot that ends a run is punctuation (a sentence full stop) or a separator
    (a dotted filename suffix), never part of the version.
    """
    return run.rstrip(".")


def _token_of(run: str) -> str | None:
    """The version token inside a scanned `run`, or None when it is not one.

    The one classification both `rewrite` (through its replace callback) and
    `stale` (through its scan) consult, so the read path never computes a
    rewrite to find out what is stale.
    """
    token = _trimmed(run)
    return token if is_version_token(token) else None


def is_version_shaped(candidate: str) -> bool:
    """True when `candidate` has at least as many dotted parts as a token.

    The near misses the rule skips (a dotted quad, a four-part build number, an
    empty component) are worth reporting; a bare integer is not.
    """
    return len(_components(candidate)) >= COMPONENTS


def _components(candidate: str) -> list[str]:
    """The dot-separated parts of `candidate`, trailing dots trimmed.

    Only `is_version_shaped` consults this: what a token *is* stays `VERSION`.
    """
    return _trimmed(candidate).split(".")


def rewrite(text: str, version: str) -> tuple[str, list[str], list[str]]:
    """Rewrite every version token to `version`.

    Returns the new text, the version tokens it replaced (in document order,
    duplicates included), and the non-token runs it left untouched.
    """
    tokens: list[str] = []
    left_alone: list[str] = []

    def replace(match: re.Match[str]) -> str:
        raw = match.group()
        token = _token_of(raw)
        if token is None:
            left_alone.append(raw)
            return raw
        tokens.append(token)
        return version + raw[len(token) :]

    return RUN.sub(replace, text), tokens, left_alone


def stale(text: str, expected: str) -> list[str]:
    """The distinct version tokens in `text` that are not `expected`, sorted.

    A scan over the runs, not a rewrite: `check` is a read-only query and
    never computes a rewritten document to answer it.
    """
    tokens = {_token_of(raw) for raw in RUN.findall(text)}
    return sorted({token for token in tokens if token is not None and token != expected})


def _report_left_alone(path: Path, left_alone: list[str]) -> None:
    """Name the version-shaped runs the sweep did not rewrite.

    A bare integer is not worth a release-log line; a dotted quad (`0.0.0.0`)
    or a four-part number is, because a reader of the sweep would otherwise
    wonder why it stayed.
    """
    near_misses = {raw for raw in left_alone if is_version_shaped(raw)}
    if not near_misses:
        return
    print(
        f"version_tokens: {path}: skipped non-version dotted run(s): {', '.join(sorted(near_misses))}",
        file=sys.stderr,
    )


class _Unreadable(Exception):
    """`_read` could not decode a file; the message names the cause."""


def _read(path: Path) -> str:
    """Read `path` as UTF-8, raising `_Unreadable` rather than guessing at the failure."""
    try:
        return path.read_bytes().decode("utf-8")
    except (OSError, UnicodeDecodeError) as e:
        raise _Unreadable(str(e)) from e


def _readable(files: list[Path]) -> Iterator[tuple[Path, str | None]]:
    """Yield `(path, text)` for every readable file, `(path, None)` for each
    unreadable one — naming it on stderr and moving on, so one bad path never
    hides the rest of the sweep. Both `rewrite_files` and `check_files`
    consume this; the caller owns the failure flag (both exit 2 on unreadable).
    """
    for path in files:
        try:
            text = _read(path)
        except _Unreadable as e:
            print(f"version_tokens: {path}: cannot read: {e}", file=sys.stderr)
            yield path, None
            continue
        yield path, text


def rewrite_files(files: list[Path], version: str) -> int:
    """Rewrite every file in place; name each failure and report what was left alone.

    A file that cannot be read is named and the sweep moves on to the next one,
    so a single bad path neither hides the remaining files nor leaves the caller
    guessing which ones were rewritten; the exit code is 2 once the sweep is
    over. `scripts/release-bump.sh` turns that into its restore.
    """
    failed = False
    for path, original in _readable(files):
        if original is None:
            failed = True
            continue
        updated, _tokens, left_alone = rewrite(original, version)
        if updated != original:
            try:
                path.write_bytes(updated.encode("utf-8"))
            except OSError as e:
                print(f"version_tokens: {path}: cannot write: {e}", file=sys.stderr)
                failed = True
        _report_left_alone(path, left_alone)
    return 2 if failed else 0


def check_files(files: list[Path], expected: str) -> int:
    """Exit 1 (after naming them) when a file still carries a stale token.

    Like `rewrite_files`, it names every file that fails and visits the rest
    before returning: 1 for stale tokens, 2 when a file was unreadable.
    """
    stale_found = False
    unreadable = False
    for path, text in _readable(files):
        if text is None:
            unreadable = True
            continue
        found = stale(text, expected)
        if found:
            print(
                f"version_tokens: {path}: version token(s) that are not {expected}: {', '.join(found)}",
                file=sys.stderr,
            )
            stale_found = True
    if unreadable:
        return 2
    return 1 if stale_found else 0


def _version(value: str) -> str:
    """argparse type: the bare `X.Y.Z` both subcommands require."""
    if not VERSION.fullmatch(value):
        raise argparse.ArgumentTypeError(f"expected a bare X.Y.Z version, got {value!r}")
    return value


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="version_tokens.py",
        description="Rewrite and check standalone product version tokens.",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    rewrite_cmd = commands.add_parser(
        "rewrite", help="rewrite every version token in FILE to --to, in place"
    )
    rewrite_cmd.add_argument("--to", required=True, type=_version, metavar="X.Y.Z")
    rewrite_cmd.add_argument("files", nargs="+", type=Path, metavar="FILE")

    check_cmd = commands.add_parser(
        "check", help="exit 1 when FILE carries a version token that is not --expect"
    )
    check_cmd.add_argument("--expect", required=True, type=_version, metavar="X.Y.Z")
    check_cmd.add_argument("files", nargs="+", type=Path, metavar="FILE")

    args = parser.parse_args(argv)
    if args.command == "rewrite":
        return rewrite_files(args.files, args.to)
    return check_files(args.files, args.expect)


if __name__ == "__main__":
    sys.exit(main())
