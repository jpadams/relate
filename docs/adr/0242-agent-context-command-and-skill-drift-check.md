# ADR-0242: `agent-context` discovery command and `skill list`/`skill check`

Status: Accepted

## Context

(Numbered ADR-0229 when written; renumbered to avoid a collision
with the ontology ADR ADR-0229 that landed on main first.)

LABS-236 (agent-CLI audit, `docs/agent-cli-audit.md` finding 9; checklist
1.3/1.4). An agent driving the cypherlite CLI has no discovery surface: no
way to ask the binary itself what commands, flags, and exit codes it
supports. neo4j-cli ships `agent-context`, but its output is ~405 KB —
a discovery document that itself violates the compactness rule. And there
is no drift detection for the installed agent skill: nothing answers "is
the skill on disk the one this binary would install?"

Two properties matter most: the readout cannot drift from reality (a
hand-maintained document would), and it must stay small enough to paste
into a model context.

## Decision

1. **`cypherlite agent-context [COMMAND]`** is a positional lifecycle
   command (ADR-0069) in the `agent-context` Cargo feature, bundled into
   `full` and `agent` (ADR-0104); in a `minimal` build the word is a
   not-included command like any other compiled-out verb. With no argument
   it prints a compact index: the compiled-in commands (word, synopsis,
   one-liner), the query-run flag names, and the exit-code table. With a
   command word it prints that command's detail: synopsis, flags, and
   examples. `update` resolves to `upgrade`.
2. **It renders from the same `COMMANDS` descriptor table that feeds
   `--help`** (ADR-0190's single source of truth) plus one `EXIT_CODES`
   constant; there is no second copy of the command list to drift. Output
   is always compact text — the command exists for machine/agent
   consumption, so there is no TTY split (contrast ADR-0240).
3. **`skill list`** prints each target agent's install state (installed /
   legacy / missing / foreign) with its path; **`skill check`** verifies
   every installed bundle against the bundle embedded in the running
   binary — missing, differing, or extra files are reported and exit 1;
   clean installs exit 0. Both accept `--agent NAME`; neither accepts
   `--force`. The comparison normalizes the `SKILL.md` `version:` line the
   same way install stamps it, so a same-binary install is always clean.
4. **Docs, help text, and tests land in the same change**, including the
   bundle table in `docs/api/cli/spec.md`.

## Alternatives considered

- **A static `agent-context` document baked at build time** — rejected:
  it would drift from `COMMANDS` the first time a command or flag
  changes; rendering from the descriptor table makes drift a compile-time
  impossibility for commands and a test failure for flags (the existing
  pins tie the flag lists to the help pages).
- **`agent-context --json`** — rejected for now: the text is already
  line-oriented and stable; a JSON form can be added behind the same
  command if a consumer needs it, without changing the default.
- **`skill check` auto-repairs drift** — rejected: silent mutation from a
  read-shaped command violates the install/remove ownership rules
  (ADR-0261/ADR-0262); the fix is an explicit `skill install`.
- **Shipping `agent-context` in `minimal` too** — rejected: `minimal` is
  the smallest query-only shell (ADR-0104); discovery belongs with the
  bundles that target operators and agents.

## Consequences

- Agents can discover the CLI from the CLI: `cypherlite agent-context`
  stays in the low-KB range by construction (one line per command and
  flag).
- `COMMAND_WORDS` grows `agent-context` in every build; a database of that
  name stays reachable via `--database agent-context`.
- The `full` and `agent` bundle definitions and the spec's bundle tables
  gain `agent-context`.
- `skill check` gives `upgrade` users a post-hoc verification path and
  agents a pre-flight sanity check.
