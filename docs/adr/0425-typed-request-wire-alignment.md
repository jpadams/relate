# ADR-0425: The typed request wire follows the real Query API — v1.2 envelopes, plain JSON stays plain

Status: Proposed
Claim: 4df4360904944100c47c59e38c426ffa11288f3c

## Context

The push work (ADR-0401, PR #463) empirically mapped the Query API v2
typed **request** wire against real servers (Neo4j 5.26, 2025.10,
2026.09-enterprise, and Aura):

- `Content-Type: application/vnd.neo4j.query.v1.2` with
  `{"$type": …, "_value": …}` envelope parameters is the real typed
  request dialect — containers included (every parameter value an
  envelope, `rows` a `List` envelope of `Map` envelopes).
- The **unversioned** `application/vnd.neo4j.query` request
  content-type is rejected (400) by every server tested.
- Under plain `application/json`, an envelope-shaped object arrives as
  a **map** — real servers pass it through, they do not coerce it.

Our `cypherlite-http` server (ADR-0236) diverges in both directions:
`typed_params.rs` decodes envelope parameters **unconditionally** —
under plain `application/json` our server coerces what every real
server passes through as a map, and it never discriminates by request
content-type at all (the origin guard checks media types only for
browser-shaped requests, and its vocabulary names the unversioned
spelling). A client developed against our serve therefore works here
and fails against the real remote — exactly the trap that produced
PR #463's invented wire (LABS-475's origin).

LABS-475 tracks the whole conformance gap; this ADR records the
request-side alignment decision.

## Decision

1. **The request media-type vocabulary** is `application/json` (plain
   parameters) and the versioned Query API family
   `application/vnd.neo4j.query.v1.2` (envelope parameters), enforced by
   the statement routes' dialect resolution. The browser guard owns only
   the CSRF property — it rejects browser-"simple" content types
   (`text/plain`, the form types, none) and passes everything else
   through to the route, so the wired surface answers a bad request type
   with the route's 400, not a pre-empting 415. The **unversioned
   `application/vnd.neo4j.query` is not a request media type**: a
   request carrying it fails closed with 400, its message naming the
   versioned spelling — matching every real server tested.

2. **Envelope decoding is bound to the request media type.** The one
   parameters fold (`typed_params`) takes the request dialect: the
   versioned typed request decodes `{"$type","_value"}` envelopes
   (today's decoder, unchanged); a plain `application/json` request
   uses plain JSON semantics — an envelope-shaped object is a user
   **map**, byte-for-byte what a real server delivers to Cypher.

3. **MCP parameters are plain.** The MCP tools' parameters ride the
   MCP protocol's JSON — plain semantics; an envelope-shaped tool
   argument stays a map, exactly as over `application/json`.

4. **The response side is unchanged** — the typed Accept negotiation
   (the versioned ladder, ADR-0236) is the real, verified response
   wire and stays as is.

5. **The conformance lane** (LABS-475 item 3): a gated test drives the
   same request shapes at our serve and at a dockerized real Neo4j and
   asserts identical accept/reject/decode behavior, so a future wire
   divergence fails CI instead of shipping.

## Alternatives considered

- **Keep the lenient decode (envelopes under any JSON request)** —
  rejected: it is precisely the trap (works against us, fails against
  every real server); LABS-475 exists because of it.
- **Accept the unversioned media type as an alias** — rejected: no
  real server accepts it as a request type; an alias re-creates the
  trap in the other direction.
- **Reject envelope parameters entirely (plain-only requests)** —
  rejected: the v1.2 envelope dialect is real, verified, and what
  official clients send; our own push client sends it.

## Consequences

- Our serve's request acceptance matches the real remote's in every
  probed direction — a client that works against `cypherlite serve`
  works against Aura (the conformance lane keeps it that way).
- Clients that relied on envelope parameters under plain
  `application/json` (none known — our surfaces send v1.2 or plain)
  must move to the versioned content type.
- ADR-0236's request-side note gets a Later-references pointer here;
  the browser guard's media-type vocabulary update rides this change.
