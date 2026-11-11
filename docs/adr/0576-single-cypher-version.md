# ADR-0576: One Cypher version, no selection surface

Status: Proposed
Claim: 4d77d7d5dffdf38e77ac1f91b93ef4d9fe5f55c2

## Context

Relate implements one Cypher dialect — the modern, GQL-aligned grammar,
Cypher 25 semantics the target where sources disagree (ADR-0002) — yet
carries three version-selection surfaces, none selecting anything real:

- The statement preamble: `CYPHER 5` or `CYPHER 25` (the two versions a
  reference server ships), typed on the statement and read by the front
  end's Cypher-5 restriction gates; its version-less options select nothing.
- The administration command `ALTER DATABASE <name> [IF EXISTS] SET
  DEFAULT LANGUAGE CYPHER 5|25`, the one database administration command
  ADR-0116 §2 kept from Neo4j Community Edition.
- The stored `defaultLanguage` of ADR-0116 §8: written by the command,
  shown by `SHOW DATABASES` (§6), consulted by nothing — §8 records it
  "does not select the parser version".

The engine pays to maintain a version it does not have. Neo4j needs the
surface for its two dialects and its configurable DBMS; an embedded
single-dialect engine has neither.

## Decision

1. **One version.** Every statement runs the engine's one dialect, the
   latest Cypher (ADR-0002's target); nothing selects another.
2. **The version prefix is removed.** A statement opening with a `CYPHER`
   version prefix fails with one authored typed error, before any other
   processing:

   `the CYPHER version prefix is no longer supported. Every statement runs the latest Cypher; please remove the prefix and resend the statement`

3. **Every `ALTER DATABASE` form fails 51N27.** `SET DEFAULT LANGUAGE`
   joins ADR-0116 §2's other administration commands in their rejection:
   `UnsupportedAdministrationCommand`, `Unsupported administration
   command: <statement>`, echoed verbatim without a leading `CYPHER n`.
   The supported set is §2's SHOW commands alone.
4. **`defaultLanguage` is a constant.** `SHOW DATABASES` keeps the column
   (ADR-0142's row shape); the value is always `CYPHER 25`, never stored.
5. **Old stores.** A default-language value recorded in an old store's
   catalog — written by an earlier `ALTER DATABASE` or an older binary —
   is ignored on load; no catalog, WAL or snapshot format changes.

## Alternatives considered

- **Keep the surfaces as recorded no-ops (today's state).** Rejected: a
  surface that parses, stores and displays a choice that cannot exist is
  theater, and every layer keeps threading a version nothing honors.
- **Honor the selection: maintain a real second dialect.** Rejected:
  ADR-0002 already picks one semantic target, and a second dialect
  doubles the version-sensitive validation arms and the test matrix for
  a dialect no query needs.
- **Reject the prefix with the parser's generic syntax error.** Rejected:
  a ported query fails unexplained; the removal carries an authored text
  naming what was removed and the single-version behavior, the house
  style of ADR-0229 §2's authored errors.
- **Keep `SET DEFAULT LANGUAGE` as the one survivor.** Rejected: with the
  column constant and the stored value ignored, the command writes
  catalog state nothing reads — the exact theater this ADR removes.
- **Drop the `defaultLanguage` column from the SHOW row.** Rejected: the
  row shape is pinned for browser and driver compatibility (ADR-0142);
  a constant value costs nothing, a missing column breaks consumers.

## Consequences

- Breaking: statements opening with a CYPHER version prefix and every
  `ALTER DATABASE` form now fail; `defaultLanguage` always reads
  `CYPHER 25`, and an older binary's store loads with its value ignored.
- The version field threaded from the parser through lowering and the
  plan, and the front end's Cypher-5 gates that read it, become dead;
  the cutover removes them with the preamble's version arm.
- ADR-0116 is superseded in part — §2's supported-command list, §6's
  stored `defaultLanguage`, §8's stored-and-shown value — and carries a
  Later references note; ADR-0002's target stands.
- Documentation changes with the cutover: the administration-command
  sections of the CLI and syntax references, and the skill bundle.
