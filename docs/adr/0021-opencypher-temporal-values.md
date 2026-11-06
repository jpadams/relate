# 0021. OpenCypher temporal values are first-class `Value`s

- **Status**: Accepted
- **Date**: 2026-08-29
- **Deciders**: MemoryDB maintainers

## Context

The pinned openCypher TCK 2024.3 (ADR-0056) includes ~1000 scenarios for
`date`, `time`, `localtime`, `datetime`, `localdatetime`, and `duration`,
plus `date.truncate` / `duration.between` and component access
(`dur.days`). MemoryDB’s `Value` enum had no temporal variants; those
scenarios could not pass. Phase 4 is the TCK 100% gate, so the dialect
must include this type family.

Temporal values are also stored as node/relationship properties and must
round-trip through tagged JSON (ADR-0005).

## Decision

Add a first-class `Value::Temporal` carrying the six openCypher temporal
kinds. Construction, comparison, component access, and duration arithmetic
follow the TCK, not Neo4j extensions beyond that corpus.

In-memory and checkpoint encoding:

- Calendar types store civil fields (and UTC offset / optional IANA name
  for zoned `datetime` / `time`).
- `duration` stores Cypher’s three-component model: months, days, and
  nanoseconds (hours/minutes/seconds fold into nanoseconds).
- Tagged JSON (`t: "date" | "time" | …`) extends ADR-0005; this is a
  `manifest.json` format_version bump when checkpoints first contain
  temporal properties.

Dotted names (`date.truncate`, `duration.between`) are ordinary function
names in the parser, not a new namespace. `EXISTS { pattern }` is a
pattern-predicate form required by the same TCK pin (not a new remote
protocol).

Implementation uses `chrono` (already a workspace crate) and `chrono-tz`
for named time zones that appear in the TCK.

## Alternatives considered

- **Store temporals as ISO-8601 strings** — `date()` vs string compare
  wrongly, and `+ duration` cannot be typed. Rejected.
- **Depend on Neo4j’s temporal library / Java time** — not embeddable in
  the Rust engine. Rejected.
- **Skip temporal and claim TCK 100% on a reduced corpus** — forbidden by
  ADR-0056 / ADR-0006. Rejected.
- **IANA zones only as fixed offsets** — TCK uses names such as
  `Europe/London`. Rejected; `chrono-tz` is required for those rows.

## Consequences

- Public Rust `Value` grows a `Temporal` variant; CLI/C ABI text export
  uses the TCK ISO-8601 literal (quoted in result tables).
- Checkpoints that persist temporal properties need a format_version bump
  (ADR-0005).
- Planner/executor must treat temporal functions as scalar, not
  aggregations.

## Later references

- **ADR-0446** names which relation each consumer uses. "Comparison follows
  the TCK" is three relations, not one: `=` (local wall clock + zone),
  ordering (instant) and the `DISTINCT` equivalence (instant + zone). Every
  equality *bucket* — an index seek, the uniqueness probe, `MERGE` matching —
  must coarsen `=` through `Temporal::datetime_eq_identity`, or a zoned
  datetime's DST fall-back overlap gets the wrong answer.
