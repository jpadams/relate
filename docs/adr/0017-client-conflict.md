# 0017. Write-write conflicts are returned to the client; the engine does not retry

- **Status**: Accepted
- **Date**: 2026-08-28
- **Deciders**: MemoryDB maintainers

## Context

ADR-0015 evaluates optimistic writes at WAL append and, on a write-set
conflict, retried the statement inside the engine (up to 64 times) before
surfacing a semantic error. That retry holds the client connection and
hides contention: latency looks like a slow commit, and the caller cannot
choose backoff, abort, or a different statement. Application clients
already own retry for network and other transient errors.

## Decision

A write-set conflict at WAL append is **`Error::Conflict`**. The engine
does not retry. The overlay is discarded; nothing is appended to the WAL.
The client retries, aborts, or compensates.

The Display string is `write-write conflict` so the C ABI
(`MEMORYDB_ERROR` + `memorydb_error_message`) and language wrappers can
detect it without a new status code.

## Alternatives considered

- **Keep in-engine retry (ADR-0015)** — fewer errors at the loadtest
  boundary. Rejected; it stalls the connection and removes client control.
- **New C status `MEMORYDB_CONFLICT`** — cleaner for bindings. Rejected
  for this change; wrappers already throw on `MEMORYDB_ERROR`. A status
  code can follow without changing this error variant.
- **Queue the writer until the conflict clears** — not optimistic. Rejected.

## Consequences

- ADR-0015’s in-engine retry loop is superseded.
- Concurrent `SET` on the same node: first committer wins; others get
  `Conflict` immediately (ADR-0015 first-committer-wins unchanged).
- Disjoint `CREATE`s still do not conflict.
- Bolt/HTTP/CLI print the same error string. Loadtest and other tools
  retry only if they choose to.

## Later references

ADR-0042: a logical memory cap is `Error::Capacity { used, limit }` at
the same WAL serialize point. The engine does not retry; Display is
`capacity exceeded: used={used} limit={limit}`.
