# Website update brief: CLI agent-friendliness changes

For the agent that maintains the Relate website. This lists what changed in
the `relate` CLI surface in this release train (LABS-229, 230, 231, 238, 242,
from the agent-CLI audit), what each page needs, and the exact wording and
examples to use. Every example below was run against the binary; copy output
verbatim, do not retype it.

Linear IDs (LABS-nnn) are for your tracking only; do not put them on public pages.
The source of truth is in the repo. If this brief and the repo disagree, the
repo wins:

| What | Where |
| --- | --- |
| Full contract | `docs/api/cli/spec.md` (sections *Query-run errors*, *Row cap*, *Result envelope*, *Help and usage errors*) |
| Flag table | `docs/reference/cli-flags.md` |
| Runnable examples | `docs/api/cli/examples/agents.md`, `docs/api/cli/examples/help.md` |
| Guides | `docs/guides/output-formats.md`, `docs/reference/dot-commands.md` |
| Decisions | `docs/adr/0192` … `0196` |

## The five changes

| # | Change | ADR | Linear |
| --- | --- | --- | --- |
| 1 | `Did you mean '--read-only'?` after an unknown flag | 0192 | LABS-242 |
| 2 | `--version --json` prints one JSON object | 0193 | LABS-238 |
| 3 | Exit-code map and a JSON error object on stdout | 0194 | LABS-229 |
| 4 | `--mode envelope` prints `{columns, rows, truncated}` | 0195 | LABS-231 |
| 5 | `--max-rows` and a 100-row default cap under agent harnesses | 0196 | LABS-230 |

### 1. Did-you-mean

An unknown flag adds one line between the error and the usage line when a real
flag is close. It applies to the query run and to every command's own flags. A
word that is not close to any flag, or a rejected positional, gets no
suggestion.

```text
Error: unknown flag: --readonly
Did you mean '--read-only'?
Usage: relate [OPTIONS] [DATABASE] [QUERY]
Run 'relate --help' for more information.
```

### 2. Version as JSON

```text
$ relate --version --json
{"arch":"aarch64","name":"relate","os":"macos","version":"0.11.7"}
```

`arch` and `os` describe the machine the binary runs on, so do not present the
values as fixed. The plain `relate --version` line is unchanged
(`Relate v0.11.7`). Use the real release number at publish time.

### 3. Exit codes and the error object (BREAKING for exact-`1` checks)

The exit-code map is shared with `neo4j-cli`:

| Exit | `code` | When |
| --- | --- | --- |
| 0 | | success, including an empty result |
| 1 | `fatal_error` | store open or I/O error, capacity, execution failure, internal error, failed final checkpoint |
| 2 | `usage_error` | unknown flag, bad or missing value (unchanged) |
| 5 | `conflict` | write-write conflict, or the store is locked by another opener |
| 6 | `validation_error` | the statement is rejected: syntax, semantic or type error, including a write on a `--read-only` database |
| 3, 4, 7, 8 | | reserved, not produced yet |

In `json`, `jsonl` and `envelope` modes a failure also prints one object on
stdout, after any rows already written. The stderr line is unchanged.

```text
$ relate --json :memory: "RETRN 1"; echo "exit=$?"
syntax error: parse error: unexpected token "RETRN"   RETRN 1   ^
{"error":{"code":"validation_error","exit_code":6,"kind":"syntax","message":"syntax error: parse error: unexpected token \"RETRN\"   RETRN 1   ^","retryable":false}}
exit=6
```

(The first line is stderr, the second stdout.) `kind` is one of `syntax`,
`semantic`, `type`, `execution`, `conflict`, `capacity`, `storage`, `locked`,
`io`, `internal`; `retryable` is `true` only for a write-write `conflict`.

**Call this out prominently wherever exit codes are documented.** Before this
change every runtime failure exited `1`. A script that tests `$? -eq 1` now
sees `6` for a rejected statement and `5` for a conflict or locked store.
Zero / non-zero checks are unaffected. Usage errors and the lifecycle commands
(`clone`, `pull`, `info`, `skill`, `upgrade`, `telemetry`, `lab`) keep their
existing codes and output.

### 4. Result envelope

```text
$ relate --mode envelope :memory: "RETURN 1 AS a, [1, 2] AS l"
{"columns":["a","l"],"rows":[{"a":1,"l":[1,2]}],"truncated":false}
$ relate --mode envelope :memory: "MATCH (n:Nope) RETURN n.p AS p"
{"columns":["p"],"rows":[],"truncated":false}
```

Say these three things: an empty result still lists its columns; `--json` and
`--jsonl` are unchanged; and keys inside each row object are sorted
alphabetically (as in `--json`), so `columns` is where the query's column order
lives. The mode is spelled `--mode envelope` only; there is no `--envelope`
shorthand and no `--format` flag yet.

### 5. Row cap (opt-out documented)

Under a detected agent harness with stdout **not** a terminal, a statement prints
at most 100 rows, writes a warning to stderr and still exits `0`. Terminals,
interactive shells and scripts outside a harness are unchanged.

```text
$ CLAUDECODE=1 relate --jsonl :memory: "UNWIND range(1, 250) AS i RETURN i" | wc -l
warning: result truncated to 100 rows (the statement returned 250); pass --max-rows 0 for all rows
100
$ CLAUDECODE=1 relate --jsonl --max-rows 0 :memory: "UNWIND range(1, 250) AS i RETURN i" | wc -l
250
$ relate --mode envelope --max-rows 2 :memory: "UNWIND range(1, 5) AS i RETURN i"
{"columns":["i"],"rows":[{"i":1},{"i":2}],"truncated":true}
```

| Control | Effect |
| --- | --- |
| `--max-rows N` | rows printed per statement; `0` is unlimited; honored everywhere, including on a terminal |
| `RELATE_MAX_ROWS=N` | the same as a default; the flag overrides it; a bad value exits 2 |

Detected harness variables (non-empty): `CLAUDECODE`, `CLAUDE_CODE`, `REPL_ID`,
`GEMINI_CLI`, `CODEX_SANDBOX`, `CODEX_THREAD_ID`, `OPENCODE`, `AUGMENT_AGENT`,
`GOOSE_PROVIDER`, `CURSOR_AGENT`; plus a case-insensitive `devin` in `EDITOR`,
`kiro` in `TERM_PROGRAM`, or `.pi/agent` in `PATH`.

State the limit plainly: the cap bounds what is **printed**, not what the engine
computes (the engine builds the full result first). A Cypher `LIMIT` is how to
bound the work. There is no `--output-file` yet, so the way to keep the full
result is `--max-rows 0` with a shell redirect.

## Pages to update

Paths are the website's pages generated from `docs/`; adjust to the site's
actual routes.

| Page | Update |
| --- | --- |
| CLI reference / flags | Add `--max-rows N`; add `envelope` to `--mode`; `--version` note (`--json`); the exit-code table with 5 and 6; the `RELATE_MAX_ROWS` row in the environment-variable table. Source: `docs/reference/cli-flags.md`, `docs/api/cli/spec.md`. |
| CLI usage errors | One example with the `Did you mean` line. Source: `docs/api/cli/examples/help.md`. |
| Output formats guide | New sections *Result envelope*, *Errors in the JSON modes*, *Row cap for agents*; add `envelope` to the format table and the `--mode` list. Source: `docs/guides/output-formats.md`. |
| Interactive shell / dot-commands | The piped-stdin failure no longer exits `1` unconditionally: it exits with the failing statement's code. Source: `docs/reference/dot-commands.md`. |
| "Use Relate from an agent" (new page, or a section of the CLI page) | One page that gathers the agent-facing behavior: the envelope, the error object and exit codes, the row cap and how to lift it, `--read-only` for exploration, `--version --json`. Source: `docs/api/cli/examples/agents.md` (all outputs verified). |
| Agent skill page | The skill bundle text changed: exit codes `5`/`6`, the error object, `--mode envelope`, `--max-rows`, and the row-cap note in `references/agent-safety.md`. Regenerate from `surfaces/relate-cli/src/skill/bundle/`. |
| Changelog / release notes | Use `CHANGELOG.md` `[Unreleased]`: one *Changed* entry (exit codes, flagged as breaking for exact-`1` checks) and four *Added* entries. |
| `llms.txt` / docs homepage | If either lists CLI capabilities or flags, add `--max-rows` and `--mode envelope`. |
| Installation / getting started | No change needed unless it shows exit codes or the `--mode` list. |

## Things not to claim

- No `--format` or `-o/--output` flag: the spelling is `--mode` (and
  `--json`, `--jsonl`, `--csv`, `--markdown`). A `--format` alias is tracked as
  LABS-232 and is not shipped.
- No `--timeout`, `--dry-run`, `agent-context`, tee-on-failure or
  `--output-file` yet (LABS-234, 241 and others are open).
- Writes are **not** gated by an agent-harness check. The harness detection
  affects only the row cap. Recommend `--read-only` for exploration.
- The error object is for query runs only. `clone`, `pull`, `info`, `skill`,
  `upgrade`, `telemetry` and `lab` do not print it.
- `columns` has the query's order; row objects do not.
- Do not describe the cap as making queries cheaper.

## Checks before publishing

1. Re-run every command in `docs/api/cli/examples/agents.md` against the
   released binary and compare output (the version line will differ).
2. A link check over any page that links into `spec.md#result-envelope`,
   `#query-run-errors` or `#row-cap`: those anchors exist as of this release.
