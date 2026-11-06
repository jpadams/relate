# 0281. Bolt session and PackStream are callable as a library

- **Status**: Accepted
- **Renumbered**: from 0159 (shared-number cleanup)
- **Date**: 2026-10-04
- **Deciders**: CypherLite maintainers

## Context

`cypherlite-bolt` is the standalone Bolt server: one bind, a first-byte
peek, WebSocket or raw Bolt, and a preset basic-auth password
(ADR-0126, ADR-0266). `handle_conn` runs the session against an
embedded `Database`, but it is `pub(crate)` inside the binary.
PackStream encode and decode live in the same binary.

MemoryDB Cloud (cloud ADR-0115) needs that session on `mdb-db-api`,
where the database already runs, and needs PackStream on the instance
ingress so the ingress can read HELLO, answer ROUTE, and relay the
other messages. The cloud must not reimplement the session.

## Decision

`cypherlite-bolt` keeps its library target and exports the session
server and PackStream:

- `handle_conn`, `read_message`, `ConnConfig`, `ConnLimits`
- `AuthConfig`, including `AuthConfig::basic`
- `Pack`, `decode`, `encode`, `try_struct_msg`, `chunk_message`,
  `routing_table`

The standalone listener, `--auth`, the generated token, and
WebSocket-on-the-same-bind stay in the binary. Exporting the session
does not add a second public protocol and does not change the wire
behavior of `cypherlite-bolt`.

## Alternatives considered

- **Copy the session into the cloud repo.** Rejected. Two Bolt
  implementations drift, and the engine tests would no longer cover
  the code the cloud runs.
- **Make the cloud shell out to the `cypherlite-bolt` binary.**
  Rejected. `mdb-db-api` already embeds `Database`. A second process
  would open the same store.

## Consequences

- Cloud `mdb-db-api` links this crate and calls `handle_conn` on a
  TLS stream it accepts itself.
- The binary and its tests call the same items through the library.
- No CLI flag changes. `docs/api/bolt/` for the standalone server is
  unchanged.
