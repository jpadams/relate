# ADR-0235: Built-in `db.ping()` procedure

Status: Accepted

## Context

Neo4j Browser probes connectivity with `CALL db.ping()` — against
CypherLite's HTTP Query API surface (ADR-0232 §3) this failed with an
unknown-procedure error, breaking Browser's connection check. `db.ping` is
a standard built-in in Neo4j (community included), so the dialect floor
arguably already implies it; the openCypher TCK does not cover it.

## Decision

Add `db.ping` as a built-in read procedure: `CALL db.ping()` yields one
column, `success` (BOOLEAN), and returns exactly one row, `success: true`
— reaching the procedure means the database answered. It is listed in
`SHOW PROCEDURES`, works on every fixed namespace (ADR-0116), and is
executable by every surface. The result shape is the connectivity contract
Browser relies on; no timing or load detail is fabricated.

## Alternatives considered

- **Do nothing; let Browser's probe fail.** Rejected: it breaks the
  connection flow of the primary browser client against our Query API
  surface for no engineering gain.
- **Return richer health data (version, uptime).** Rejected: fabricated
  telemetry would be dishonest; `success: true` is exactly what the probe
  checks, and real diagnostics belong to `db.info`/`dbms.*` procedures
  with their own documented placeholders.

## Consequences

- `SHOW PROCEDURES` output gains one listed row (`db.ping`, READ mode,
  `success: BOOLEAN` output).
- The MCP `read-cypher` tool accepts `CALL db.ping()` (it is a pure read,
  `QueryType::Read`), so agents can probe the same way.
- The TCK corpus is untouched (no scenario covers `db.ping`; the
  allowlist does not grow).

Later references: landed on the `serve-network-surfaces` branch with the
Query API media-type acceptance (the two Browser blockers shipped
together).
