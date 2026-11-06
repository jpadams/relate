# 0029. Withdraw the localhost HTTP sidecar

- **Status**: Accepted
- **Date**: 2026-08-31
- **Deciders**: MemoryDB maintainers

## Context

ADR-0008 shipped `memorydb-http` as a localhost-only JSON query API for
the explorer. The managed-service plane (`memorydb-cloud`) takes HTTPS +
JWT as the only HTTP query surface. Keeping a second HTTP stack in this
repo would split the client contract and re-litigate ADR-0057’s “HTTP is
not the public remote protocol.”

Bolt remains the optional remote transport in this repo (ADR-0057). It
is unused by cloud v1.

## Decision

Remove the HTTP product surface from MemoryDB:

- Delete the `memorydb-http` crate and `docs/api/http/`.
- Move the explorer and `POST /query` client to `memorydb-cloud` ingress.
- This repository’s public surfaces are the Rust crate, C ABI, language
  clients, CLI, and Bolt.

ADR-0008 is withdrawn here, not edited in place.

## Alternatives considered

- **Keep `memorydb-http` for local explorer** — two HTTP contracts.
  Rejected; the ingress owns JSON query.
- **Promote engine HTTP to the SaaS edge** — rejected by the cloud
  plane (JWT, CRs, NetworkPolicy live in `memorydb-cloud`).
- **Replace HTTP with in-cluster Bolt as the tenant API now** — deferred;
  cloud v1 is HTTPS + JWT only.

## Consequences

- Local visualization uses the cloud ingress (or the CLI / embed API).
- CI no longer builds or tests `memorydb-http`.
- A later return of engine HTTP would be a new ADR.
