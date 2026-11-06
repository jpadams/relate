# ADR-0383: The lab importers take `--database`, and `--db` is removed

Status: Accepted
Claim: b31aa10014d26b0d90d350fb8a098986ce2208be

## Context

ADR-0200 gives the CLI one flag for the store: `-d` / `--database PATH` on
`query`, the path-first form, `pull` and `info`; `serve`, introduced later by
ADR-0232, takes the same flag. It carved out one exception in its grammar —
"`lab fs-to-graph` keeps `--db`" — because the importer predated the grammar:
ADR-0157 had already pinned `--db` as the flag's name, with `:memory:` as its
default and the TTY shell when it was omitted.

ADR-0286 then made the explicit flag meaningful: an explicit `--db` never opens
the interactive shell, while omitting it does open it on a TTY with `repl`.
ADR-0376 followed the same precedent for the second importer and repeated the
carve-out textually ("not the statement session's `--database` (ADR-0200)").
So `cypherlite lab fs-to-graph --db PATH` and
`cypherlite lab git-to-graph --db PATH` are the only commands in the CLI that
take a store under a second flag spelling. `--db` has shipped on `fs-to-graph`
since 0.11.0 (ADR-0286 records that release) and is pinned in three accepted
ADRs, so changing it is a recorded decision, not a tidy-up.

The request is consistency: one spelling for the store everywhere, with the
short form ADR-0200 already defines. That means `--database PATH` and `-d PATH`
on both importer actions, and `--db` removed rather than kept as an alias.

The workspace is 0.12.x, so a breaking CLI change can land in a minor release,
and the `lab` actions are visibly labelled experimental in `--help`, in
`docs/api/cli/spec.md` and in `docs/reference/cli-flags.md`. Nothing here is a
silent break: it is recorded in this ADR and in the CHANGELOG.

## Decision

1. **Both `lab` importers take `-d` / `--database PATH`.** The grammar is

   ```
   cypherlite lab fs-to-graph [DIR] [--database PATH] [--max-content-bytes N]
                              [--include-hidden] [--ignore-patterns PAT]
   cypherlite lab git-to-graph [DIR] [--database PATH] [--max-commits N]
                               [--max-changeset N] [--since DATE]
   ```

   `-d` is ADR-0200's short form of `--database` — "the one new single-dash
   flag" in that ADR's words, beside the pre-existing single-dash aliases
   (`-json`, `-mode`, …) that AGENTS.md §3 records as legacy — and it means
   the same thing on both actions. The target still defaults to `:memory:`,
   and an explicit flag — either spelling — still carries ADR-0286's rule:
   print the summary, checkpoint a persistent target, and exit without opening
   the interactive shell. Omitting the flag is unchanged. This is a flag
   rename; the value handling, the default and the exit behavior are not.

2. **`--db` is removed rather than aliased.** `cypherlite lab fs-to-graph --db
   PATH` and `cypherlite lab git-to-graph --db PATH` become unknown-flag usage
   errors: the `lab` usage line and a `--help` hint, exit 2, with no value
   consumed and no migration hint — the same treatment AGENTS.md §3 gives the
   removed `--skill` / `--agent` spellings. One spelling is the point.

3. **The supersession is the flag spelling only.** This ADR supersedes the
   `--db` spelling in ADR-0157, ADR-0286 and ADR-0376, and nothing else in
   them: not the graph schema, walk semantics, content heuristic, index or
   `:memory:` default of ADR-0157; not ADR-0286's explicit-versus-omitted exit
   rule, which now keys on whether `-d` / `--database` was passed; not
   ADR-0376's schema, path rule, bounds or idempotence. Those ADRs each gain a
   **Later references** note naming this ADR.

4. **The experimental `lab` namespace can carry the break.** `lab` exists for
   commands that manipulate a graph from outside the engine (ADR-0279), and both
   visible actions are documented as experimental. ADR-0279's hidden-action
   policy does not cover them — they are visible, tested and changelogged — but
   the experimental label is the compatibility warning their users already have,
   and 0.12.x allows a breaking change in a minor release. The alternative was
   to leave the CLI with two store spellings indefinitely, which is the problem
   being fixed.

## Alternatives considered

- **Keep `--db` as a hidden alias.** Rejected: a hidden spelling still parses,
  still needs tests, and still has to be documented or deliberately omitted
  from docs that a visible action's users will read; it would leave two store
  spellings exactly where the request was one.

- **Rename only `git-to-graph`.** Rejected: it is the newer action and the
  smaller diff, but `fs-to-graph` is the action the skill catalog pairs it
  with, so the documented workflow (`fs-to-graph` then `git-to-graph`) would
  still make a user type both spellings in one session.

- **Keep `--db` and add `--database` as a second documented spelling.**
  Rejected: two documented spellings for one store mean every usage line, flag
  table, reference row and test carries both, and the next reader has to decide
  which is canonical; it is the hidden alias with more documentation debt.

- **Print a deprecation warning and keep `--db` for one release.** Rejected:
  the CLI has no deprecation-warning mechanism, and a warning release still
  ships both spellings (the same two-spelling cost) without changing the
  eventual removal.

- **Rename the flag only at the next major.** Rejected: the workspace is
  0.12.x, so there is no major boundary to wait for, 1.0 has no date, and the
  experimental label already discloses the break — waiting would outlive every
  lab release without making the old spelling any more stable.

## Consequences

- **Parser.** `surfaces/cypherlite-cli/src/lab_cmd.rs` accepts `-d` /
  `--database` on both actions through one shared value helper, so value
  handling, error text and the `-`-prefixed-path rule (ADR-0200 §3) cannot
  drift between them; `--db` falls through to the unknown-flag path with exit 2.

- **Hand-written help.** `src/help/lab.rs` replaces both usage lines and both
  per-action flag lists, and `src/help/mod.rs` updates the `lab` synopsis and
  examples. `--help` is the source of truth for this parser (AGENTS.md §3).

- **Spec and reference.** `docs/api/cli/spec.md` updates both usage lines, both
  command-table rows, both `lab` sections' flag tables and every copy-paste
  invocation; `docs/reference/cli-flags.md` updates the two `lab` rows, their
  examples and the explicit-flag prose. `AGENTS.md`'s commands-vs-flags block
  lists both actions with `--database PATH`.

- **Changelog.** `CHANGELOG.md` gets an `Unreleased` entry naming the break:
  `--db` becomes `-d` / `--database` on both importers and `--db` now exits 2.

- **Skill catalog.** `src/skill/bundle/references/lab-git-to-graph.md` updates
  its usage line, both examples and its explicit-`--db` prose; the BUNDLE_FILES
  drift guard keeps the bundle's file list honest.

- **Tests.** `tests/e2e_lab.rs`, `tests/e2e_lab_git.rs` and
  `tests/e2e_lab_git_catalog.rs` switch every spawned `--db` to `-d` or
  `--database`, including the argv built by the PTY driver in
  `tests/support/mod.rs` and the flag named in `pty_lab_shell.py`; a new case
  pins `--db` to exit 2 with the usage line and no import.

- **`--db` now errors.** It is an unknown flag, not a deprecated spelling: exit
  2, usage line, `--help` hint, no migration message, and no partial import.

- **Unchanged contract.** The graph schemas, path rules, bounds, index names,
  `:memory:` default and ADR-0286's exit rule stand. A script that omits the
  flag behaves exactly as before; only a script that passes `--db` must change.
