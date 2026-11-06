# 0118. CLI `--namespace` selects the session namespace

- **Status**: Accepted
- **Date**: 2026-09-27
- **Deciders**: CypherLite maintainers (Linear LABS-102)

## Context

ADR-0116 adds the fixed namespaces `neo4j`, `system` and `system_ontologies`,
and ADR-0117 lets the embedding APIs and Bolt choose a connection's session
namespace. The `cypherlite` CLI opens one store from its `DATABASE` positional
and can only reach another namespace through a `USE` clause in every
statement.

ADR-0069 governs the CLI's shape: flags configure a query-engine run, new
flags are double-dash kebab-case, and lifecycle actions are commands. ADR-0058
withdrew the shell's `.use` command. Neo4j's cypher-shell and neo4j-cli select
a database with `-d/--database`, but in CypherLite the positional `DATABASE`
already means the store.

## Decision

1. **Flag.** `--namespace NAME` sets the session namespace for the run. Every
   statement without `USE` targets it, in one-shot (`-c` or a positional
   query), piped and interactive use. `NAME` is `neo4j` (the default),
   `system` or `system_ontologies`, matched case-insensitively.
2. **Validation.** Any other value fails before the store is opened, the same
   way other invalid flag values do: `Error: invalid --namespace: unknown
   namespace '<name>' (expected neo4j, system or system_ontologies)`, followed
   by the usage text on stderr, with exit status 2.
3. **Scope.** The flag is part of every query run, so it is not a Cargo
   feature and exists in the `full`, `agent` and `minimal` bundles
   (ADR-0104). `pull` does not accept it, because `pull` always writes
   `neo4j`; it reports `--namespace` as an unknown flag.
4. **No short form, environment variable or dot-command.** There is no `-d`,
   which would suggest Neo4j's `--database` while `DATABASE` means the store
   here. There is no environment variable, and `NEO4J_DATABASE` keeps meaning
   `pull`'s remote source. There is no `.use` (ADR-0058); `USE` inside a
   statement covers one-off switching.
5. **Documentation.** The hand-written `--help` text and the flag table in
   `docs/api/cli/spec.md` gain the flag, its values and its default, with an
   example, in the same change as the code:

   ```sh
   cypherlite ./store.cl --namespace system_ontologies -c 'MATCH (n) RETURN count(n)'
   ```

## Alternatives considered

- **`-d/--database`, relabelling the positional as `STORE`.** Rejected: it
  matches cypher-shell, but CypherLite uses "database" for the store in the
  CLI, the Rust type and the Python API. Renaming that vocabulary is out of
  proportion to a selector with three values.
- **A `.use` dot-command.** Rejected: ADR-0058 withdrew it, and `USE` in a
  statement already switches the namespace for that statement.
- **An environment variable such as `CYPHERLITE_NAMESPACE`.** Rejected for
  now: it is hidden state that changes where an agent's queries land. It can
  be added later without breaking the flag.
- **`pull --namespace`.** Rejected: `system_ontologies` is written only by
  procedures (ADR-0121) and `system` holds no data, so the only valid target
  is the default.

## Consequences

- Agents and scripts can query the ontology or run `system` commands without
  rewriting their statements.
- The CLI gates in AGENTS.md run for all three bundles, with tests for each
  value, the default and the unknown-name error.

## Later references

ADR-0200 gives `-d` to the store flag `--database`. It is not a short form
of `--namespace`. Decision 4 still stands for the namespace flag.
