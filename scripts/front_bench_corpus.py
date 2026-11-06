#!/usr/bin/env python3
"""Build the front_bench statement corpus.

Extracts Cypher statements for parser/lowerer profiling
(relate/relate-front/examples/front_bench.rs):

- every statement of every workload file
  testing/relate-query-latency/workloads/**/*.cypher, split on
  top-level semicolons by `split_statements` below (a workload
  file may carry several statements, e.g. graph-benchmark-ldbc's
  `indexes.cypher`); `$x` parameters stay as placeholders
- triple-quoted statement blocks from an openCypher `.feature` corpus whose
  first word is a Cypher statement keyword. The corpus is not in this repo:
  point $TCK_CORPUS at the spec-harness checkout's `features/tck/` (or any
  openCypher checkout's `tck/features`); unset means workload statements
  only, stated in the output, and a named corpus with no feature file is a
  loud failure

Statements are deduplicated by text (workload-first, first occurrence)
and written as a single \\x1e-separated corpus file (default
target/front-bench-corpus.txt), the format front_bench consumes.

The Gherkin reader (`parse_feature`) and the statement splitter
(`split_statements`) below moved here from the verifier script that
retired with the corpora's move to the spec harness (ADR-0520); the
harness checkout's own `scripts/cip_verify.py` is the verifier of
record.

Usage: front_bench_corpus.py [output_path]
"""
import os
import pathlib
import sys
from dataclasses import dataclass, field
from itertools import chain
from typing import Optional

REPO = pathlib.Path(__file__).resolve().parent.parent
OUT = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else REPO / "target/front-bench-corpus.txt"

# The TCK shelf, external by design (the corpus moved to the spec-harness
# repo): no in-repo default can silently stand in for it.
TCK_ENV = "TCK_CORPUS"

KEYWORDS = (
    "MATCH", "OPTIONAL", "RETURN", "WITH", "UNWIND", "CREATE", "MERGE",
    "DELETE", "SET", "REMOVE", "FOREACH", "CALL", "USE", "SHOW", "DROP",
    "LOAD", "ALTER", "START", "STOP", "EXPLAIN", "PROFILE", "SEARCH",
    "INSERT", "WHEN", "FINISH", "PERIODIC",
)

# --------------------------------------------------------------------------
# Gherkin parse layer, relocated from the verifier script that retired
# with the corpora's move to the spec harness (ADR-0520) — the harness
# checkout's copy is the verifier of record
# --------------------------------------------------------------------------

STEP_KEYWORDS = ("Given ", "When ", "Then ", "And ", "But ")


@dataclass
class Step:
    doc_string: Optional[str] = None


@dataclass
class Scenario:
    steps: list[Step] = field(default_factory=list)


@dataclass
class Feature:
    background: list[Step] = field(default_factory=list)
    scenarios: list[Scenario] = field(default_factory=list)


def _dedent_doc_string(lines: list[str]) -> str:
    nonblank = [ln for ln in lines if ln.strip()]
    if not nonblank:
        return ""
    indent = min(len(ln) - len(ln.lstrip()) for ln in nonblank)
    return "\n".join(ln[indent:] for ln in lines)


def parse_feature(text: str) -> Feature:
    """Parse a .feature file into a Feature (dialect of the openCypher TCK).

    The model carries only what the corpus extractor (`tck_statements`)
    reads: the feature/scenario/step walk and each step's doc string —
    step tables and Examples rows are walked past, not modeled."""
    lines = text.splitlines()
    feature = Feature()
    current: Optional[Scenario] = None
    in_background = False
    i = 0

    def parse_step_at(idx: int) -> tuple[Step, int]:
        # The walk already established lines[idx] opens a step; consume
        # the step line, then the doc string and/or table rows that may
        # follow it (in either order), keeping only the doc string.
        step = Step()
        idx += 1
        while idx < len(lines):
            nxt = lines[idx].strip()
            if nxt == '"""':
                idx += 1
                doc_lines: list[str] = []
                while idx < len(lines):
                    if lines[idx].strip() == '"""':
                        idx += 1
                        break
                    doc_lines.append(lines[idx])
                    idx += 1
                step.doc_string = _dedent_doc_string(doc_lines)
                continue
            if nxt.startswith("|"):
                while idx < len(lines) and lines[idx].strip().startswith("|"):
                    idx += 1
                continue
            break
        return step, idx

    while i < len(lines):
        line = lines[i].strip()

        if not line or line.startswith("#"):
            i += 1
            continue

        if line.startswith("Feature:"):
            i += 1
            continue

        if line.startswith("Background:"):
            in_background = True
            current = None
            i += 1
            continue

        if line.startswith("Scenario Outline:") or line.startswith("Scenario:"):
            current = Scenario()
            in_background = False
            feature.scenarios.append(current)
            i += 1
            continue

        if any(line.startswith(kw) for kw in STEP_KEYWORDS):
            step, i = parse_step_at(i)
            if in_background:
                feature.background.append(step)
            elif current is not None:
                current.steps.append(step)
            continue

        i += 1

    return feature


def split_statements(text: str) -> list[str]:
    """Split statement text on top-level semicolons, tracking single- and
    double-quoted strings for statement-boundary detection in the bench
    corpus: a `;` inside a quoted string never ends a statement, and the
    other quote kind inside a string is an ordinary character. A
    bench-corpus convenience, not parity with the canonical
    `relate-splitter` (Rust), which also models escapes, comments and
    backtick-quoted identifiers."""
    stmts: list[str] = []
    cur: list[str] = []
    # One quote state: None outside a string, the open quote character
    # inside it — at most one string is open at a time.
    quote: Optional[str] = None
    for ch in text:
        if quote is None:
            if ch in ("'", '"'):
                quote = ch
            elif ch == ";":
                stmt = "".join(cur).strip()
                if stmt:
                    stmts.append(stmt)
                cur = []
                continue
        elif ch == quote:
            quote = None
        cur.append(ch)
    stmt = "".join(cur).strip()
    if stmt:
        stmts.append(stmt)
    return stmts


def read_strict(path):
    """Read `path` as UTF-8, failing loud and naming the file — the one
    reader both collectors use, so a malformed source dies with the file
    named instead of smuggling replacement characters into the corpus."""
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError as e:
        raise SystemExit(f"{path}: not valid UTF-8: {e}") from e


def workload_statements():
    """Every statement of every workload file, split on top-level
    semicolons by `split_statements` above, never assuming one
    statement per file."""
    root = REPO / "testing/relate-query-latency/workloads"
    out = []
    for f in sorted(root.rglob("*.cypher")):
        out.extend(split_statements(read_strict(f)))
    return out


def tck_root():
    """The corpus directory named by $TCK_ENV, or None when it is unset.

    A named path must be a directory carrying at least one `.feature`
    file: a typo or an empty shelf exits loud instead of contributing an
    empty corpus, the silent-default failure this leg used to risk when
    the corpus lived at a fixed in-repo path."""
    raw = os.environ.get(TCK_ENV)
    if not raw:
        return None
    root = pathlib.Path(raw)
    if not root.is_dir():
        raise SystemExit(
            f"{TCK_ENV}={raw}: not a directory (expected the spec-harness "
            "`features/tck` or an openCypher `tck/features` checkout)"
        )
    if not next(root.rglob("*.feature"), None):
        raise SystemExit(f"{TCK_ENV}={raw}: no .feature file under this directory")
    return root


def tck_statements(root):
    """Every doc-string statement whose first word is a Cypher statement
    keyword. The `\"\"\"` doc-block walk is `parse_feature`'s — this
    file's Gherkin reader — with doc strings dedented exactly as the
    spec harness's TCK consumers read them."""
    out = []
    for f in sorted(root.rglob("*.feature")):
        feature = parse_feature(read_strict(f))
        for step in chain(feature.background, *(sc.steps for sc in feature.scenarios)):
            if step.doc_string is None:
                continue
            statement = step.doc_string.strip()
            first = statement.split(None, 1)[0].upper() if statement else ""
            if first in KEYWORDS:
                out.append(statement)
    return out


def main():
    wl = workload_statements()
    root = tck_root()
    if root is None:
        print(
            f"tck: skipped — set {TCK_ENV} to an openCypher .feature corpus "
            "(spec-harness `features/tck`, or an openCypher `tck/features`)"
        )
        tck = []
    else:
        tck = tck_statements(root)
    # One dedupe pass over the concatenation, workload-first,
    # first-occurrence order, so the append-if-unseen policy and its
    # precedence live here alone. Both per-source counts come off that
    # same pass: a statement's source is the list that contributed it —
    # a statement in both counts as a workload statement, exactly the
    # workload-first order's attribution.
    statements: list[str] = []
    seen: set[str] = set()
    counts = {"workloads": 0, "tck": 0}
    for source, stmts in (("workloads", wl), ("tck", tck)):
        for stmt in stmts:
            if stmt in seen:
                continue
            seen.add(stmt)
            statements.append(stmt)
            counts[source] += 1
    wl_count = counts["workloads"]
    tck_count = counts["tck"]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w", encoding="utf-8") as fh:
        fh.write("\x1e".join(statements))
    total_bytes = sum(len(s) for s in statements)
    print(f"workloads: {wl_count}  tck: {tck_count}  total: {len(statements)}")
    print(f"total bytes: {total_bytes}")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
