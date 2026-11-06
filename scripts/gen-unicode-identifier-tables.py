#!/usr/bin/env python3
"""Generate the Unicode identifier tables for the Relate parser (ADR-0182).

The CYPHER 25 rule for an unquoted identifier character is a BMP-only
(`code_point <= U+FFFF`) predicate over the Unicode 13.0.0
`General_Category` and `Pattern_Syntax` properties, with no exceptions:

- identifier start: `General_Category` in {Lu, Ll, Lt, Lm, Lo, Nl, Pc}
  and NOT `Pattern_Syntax`;
- identifier part: `General_Category` in {Lu, Ll, Lt, Lm, Lo, Nl, Pc, Mn,
  Mc, Nd, Sc} and NOT `Pattern_Syntax`.

The rule was pinned by probing Neo4j 2026.09 under `CYPHER 25` (LABS-188);
the tables are Unicode **13.0.0** (not Java's Unicode 16.0), and they are
deliberately not UAX #31 `ID_Start`/`ID_Continue` (U+203F is a valid start,
`Other_ID_Start` U+2118 is rejected, `Sc` is a valid part).

This script reads `UnicodeData.txt` and `PropList.txt` from
<https://www.unicode.org/Public/13.0.0/ucd/> (both SHA-256-pinned below),
applies the predicates, and writes the data-only module
`relate/relate-parser/src/unicode_identifier/tables.rs` with two
sorted inclusive BMP range tables. ASCII is decided by the hand-written
predicates in the parent module (`[A-Za-z_]` / `[A-Za-z0-9_]`, ADR-0182),
so the emitted tables start at U+0080 and hold 383 start + 436 part
ranges.

Usage
-----

Regenerate (fetches the UCD over HTTPS, verifies the pinned digests, writes
the file and prints its SHA-256 -- the drift-guard test in
`lexer_tests.rs` pins that digest):

    python3 scripts/gen-unicode-identifier-tables.py

Check the checked-in file without writing it (exits non-zero on drift):

    python3 scripts/gen-unicode-identifier-tables.py --check

Read the two inputs from a local directory instead of fetching (their
digests are still verified; useful offline, e.g. against a UCD cache):

    python3 scripts/gen-unicode-identifier-tables.py --ucd-dir /path/to/ucd
"""

import argparse
import hashlib
import sys
import urllib.request
from pathlib import Path
from typing import Callable

UCD_BASE = "https://www.unicode.org/Public/13.0.0/ucd/"

# SHA-256 of the pinned inputs at UCD_BASE, verified on every run.
INPUT_SHA256 = {
    "UnicodeData.txt": "bdbffbbfc8ad4d3a6d01b5891510458f3d36f7170422af4ea2bed3211a73e8bb",
    "PropList.txt": "485b5a3ed25dbf1f94dfa5a9b69d8b4550ffd0c33045ccc55ccfd7c80b2a40cf",
}

START_CATEGORIES = frozenset({"Lu", "Ll", "Lt", "Lm", "Lo", "Nl", "Pc"})
PART_CATEGORIES = frozenset({"Lu", "Ll", "Lt", "Lm", "Lo", "Nl", "Pc", "Mn", "Mc", "Nd", "Sc"})

BMP_MAX = 0xFFFF
ASCII_MAX = 0x7F

OUT_PATH = (
    Path(__file__).resolve().parent.parent
    / "relate"
    / "relate-parser"
    / "src"
    / "unicode_identifier"
    / "tables.rs"
)


def load_input(name: str, ucd_dir: str | None) -> str:
    """Load one pinned UCD input, verifying its SHA-256."""
    if ucd_dir is not None:
        data = (Path(ucd_dir) / name).read_bytes()
    else:
        with urllib.request.urlopen(UCD_BASE + name) as response:
            data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    expected = INPUT_SHA256[name]
    if digest != expected:
        raise SystemExit(
            f"{name}: SHA-256 mismatch\n  expected {expected}\n  got      {digest}"
        )
    return data.decode("utf-8")


def parse_general_categories(unicode_data: str) -> dict[int, str]:
    """Map code point -> General_Category from UnicodeData.txt.

    `First>`/`Last>` range pairs (CJK ideographs, Hangul syllables, ...)
    expand to every code point in the block; code points absent from the
    file are unassigned (`Cn`).
    """
    categories: dict[int, str] = {}
    lines = unicode_data.splitlines()
    i = 0
    while i < len(lines):
        fields = lines[i].split(";")
        code = int(fields[0], 16)
        name, category = fields[1], fields[2]
        if name.endswith(", First>"):
            last = int(lines[i + 1].split(";")[0], 16)
            for cp in range(code, last + 1):
                categories[cp] = category
            i += 2
        else:
            categories[code] = category
            i += 1
    return categories


def parse_binary_property(prop_list: str, property_name: str) -> set[int]:
    """The code points carrying the named binary property from PropList.txt."""
    code_points: set[int] = set()
    for line in prop_list.splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        code_range, _, name = line.partition(";")
        if name.strip() != property_name:
            continue
        code_range = code_range.strip()
        if ".." in code_range:
            first, last = code_range.split("..")
        else:
            first = last = code_range
        code_points.update(range(int(first, 16), int(last, 16) + 1))
    return code_points


def collapse_ranges(predicate: Callable[[int], bool]) -> list[tuple[int, int]]:
    """Sorted inclusive ranges of the code points in U+0080..=U+FFFF where
    `predicate` holds (ASCII is the hand-written predicates' fast path)."""
    ranges: list[tuple[int, int]] = []
    first: int | None = None
    for cp in range(ASCII_MAX + 1, BMP_MAX + 1):
        if predicate(cp):
            if first is None:
                first = cp
        elif first is not None:
            ranges.append((first, cp - 1))
            first = None
    if first is not None:
        ranges.append((first, BMP_MAX))
    return ranges


def render_ranges(const_name: str, ranges: list[tuple[int, int]]) -> str:
    lines = [
        "#[rustfmt::skip]",
        f"pub const {const_name}: &[(u16, u16)] = &[",
    ]
    lines += [f"    (0x{first:04X}, 0x{last:04X})," for first, last in ranges]
    lines += ["];"]
    return "\n".join(lines)


def render_file(
    start_ranges: list[tuple[int, int]], part_ranges: list[tuple[int, int]]
) -> str:
    return f'''//! GENERATED FILE -- do not edit by hand. Regenerate with:
//!
//! ```text
//! python3 scripts/gen-unicode-identifier-tables.py
//! ```
//!
//! The generated Unicode 13.0.0 identifier range tables (ADR-0182), used by
//! the hand-written `is_identifier_start` / `is_identifier_part` predicates
//! in the parent `unicode_identifier` module.
//!
//! Inputs (Unicode 13.0.0, SHA-256 pinned in the generator):
//!
//! - {UCD_BASE}UnicodeData.txt
//!   sha256 {INPUT_SHA256["UnicodeData.txt"]}
//! - {UCD_BASE}PropList.txt
//!   sha256 {INPUT_SHA256["PropList.txt"]}
//!
//! ADR-0182: an unquoted identifier character is a BMP code point
//! (`<= U+FFFF`) whose Unicode 13.0.0 `General_Category` is in the start or
//! part set and which is not `Pattern_Syntax`; there are no exceptions.
//! The tables start at U+0080 -- ASCII is decided by the hand-written
//! predicates in the parent module, the source of truth for ASCII.

/// The Unicode version of the tables in this file.
///
/// Read by the crate's tests and kept as the pinned version marker (ADR-0182);
/// the parser itself needs only the tables, so this is allowed to be unused
/// in the non-test build.
#[allow(dead_code)]
pub const UNICODE_VERSION: (u8, u8, u8) = (13, 0, 0);

/// Identifier-start ranges: `General_Category` in
/// {{Lu, Ll, Lt, Lm, Lo, Nl, Pc}} and not `Pattern_Syntax`.
{render_ranges("IDENTIFIER_START_RANGES", start_ranges)}

/// Identifier-part ranges: `General_Category` in
/// {{Lu, Ll, Lt, Lm, Lo, Nl, Pc, Mn, Mc, Nd, Sc}} and not `Pattern_Syntax`.
{render_ranges("IDENTIFIER_PART_RANGES", part_ranges)}
'''


def generate(ucd_dir: str | None) -> str:
    categories = parse_general_categories(load_input("UnicodeData.txt", ucd_dir))
    pattern_syntax = parse_binary_property(load_input("PropList.txt", ucd_dir), "Pattern_Syntax")

    start_ranges = collapse_ranges(
        lambda cp: categories.get(cp, "Cn") in START_CATEGORIES and cp not in pattern_syntax
    )
    part_ranges = collapse_ranges(
        lambda cp: categories.get(cp, "Cn") in PART_CATEGORIES and cp not in pattern_syntax
    )
    return render_file(start_ranges, part_ranges)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the checked-in file is exactly the generated output; write nothing",
    )
    parser.add_argument(
        "--ucd-dir",
        default=None,
        help="directory holding UnicodeData.txt and PropList.txt (default: fetch from unicode.org)",
    )
    args = parser.parse_args()

    content = generate(args.ucd_dir)
    digest = hashlib.sha256(content.encode("utf-8")).hexdigest()

    if args.check:
        # Compare raw bytes, not decoded text: the drift guard hashes the
        # checked-out bytes (`include_bytes!`), so universal-newline
        # translation on read would report a CRLF copy as matching.
        current = OUT_PATH.read_bytes() if OUT_PATH.exists() else b""
        if current != content.encode("utf-8"):
            print(
                f"ERROR: {OUT_PATH} differs from the generated output; regenerate it",
                file=sys.stderr,
            )
            return 1
        print(f"ok: {OUT_PATH} matches the generated output")
        return 0

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    # Pin LF explicitly: the drift guard hashes the raw checked-out bytes, so
    # a Windows run (where text mode would translate to CRLF) must produce the
    # same bytes as a Unix run.
    OUT_PATH.write_text(content, newline="\n")
    print(f"wrote {OUT_PATH}")
    print(f"sha256: {digest}")
    print("(update the pinned digest in relate-parser's lexer drift-guard test)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
