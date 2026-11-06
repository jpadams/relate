# ADR-0318: Bolt ROUTE answers the home database neo4j and Neo4j's WRITE/READ/ROUTE role order

Status: Proposed
Claim: 2f49a2185abec02ade710bd6add6efabec429c7c

## Context

ADR-0269 answers ROUTE with a single-server self routing table:
`rt: {ttl: 300, servers: [ROUTE, READ, WRITE entries], db: <requested db
or "system">}`. ADR-0140 later fixed the advertised address.

Drivers resolve the home database through ROUTE: when the routing request
names no database, they adopt the returned `rt.db` for later requests.
The LABS-323 probe (findings F-36, F-37) found that latest Neo4j answers
`db: "neo4j"` (the home database) when ROUTE names none, and lists the
roles in the order WRITE, READ, ROUTE. cypherlite-bolt answers
`db: "system"`, so a `neo4j://` driver would treat `system` as home and
send its queries there.

## Decision

1. ROUTE answers `SUCCESS {rt: {servers: [{addresses: [<self>], role:
   "WRITE"}, {addresses: [<self>], role: "READ"}, {addresses: [<self>],
   role: "ROUTE"}], ttl: 300, db: <db>}}`.
2. `db` is the extra map's `db` when it names one (a non-empty string),
   else the home database `"neo4j"`, the same default RUN and BEGIN use
   (ADR-0117 §7).
3. The roles come in the order WRITE, READ, ROUTE.
4. `<self>` (ADR-0140), `ttl` 300, the failed-state exemption and the
   ADR-0126 auth gate are unchanged.
5. This reverses ADR-0269's `db` default and role order.

## Alternatives considered

- **Keep `"system"` as the default.** It matched the neo4j-browser
  connect message, which names `system` explicitly anyway. Rejected:
  drivers that ask without a database would make `system` their home
  database.
- **Keep the role order.** Drivers read roles by name, so the order is
  cosmetic. Rejected: matching Neo4j byte for byte costs nothing and keeps
  wire comparisons simple.

## Consequences

- `neo4j://` drivers resolve `neo4j` as the home database. neo4j-browser,
  which sends `db: "system"`, still gets `system`.
- ADR-0269 gains a Later references note pointing here.
- Pinned by `tests/compat/scripts/protocol/route-home-db-and-role-order.bolt`
  and the ROUTE tests in `protocol_route_auth_tests.rs`.
