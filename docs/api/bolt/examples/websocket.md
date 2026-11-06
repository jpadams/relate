# Example: official JS driver over Bolt-over-WebSocket

Bolt is served over WebSocket on the same bind as raw Bolt — serve's Bolt
surface keeps WebSocket serving on (ADR-0266, ADR-0368) — so browser
clients — which cannot open raw TCP sockets — can
connect. This example uses the official Neo4j JavaScript driver's prebuilt
browser ESM bundle (`neo4j-driver/lib/browser/neo4j-web.esm.js`), i.e. the
same Bolt-over-WebSocket channel neo4j-browser uses, and runs it under
Node 22, where the bundle dials through the global `WebSocket`.

Start the server from the repository root:

```bash
cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687 --database :memory: --auth none
```

With `--auth none` the server ignores the credentials the driver sends in
HELLO, so the example passes placeholder basic-auth credentials (the driver
requires an auth token object). On a loopback bind, WebSocket upgrade
requests without an `Origin` header — such as Node's — are allowed; browsers
send an `Origin`, which `serve --ws-allow-origin` widens (default:
loopback-only with `--auth none`; with auth on, loopback plus the hosted
Neo4j Browser `https://browser.neo4j.io` and Neo4j Console
`https://console.neo4j.io` — any other Origin needs the flag; ADR-0459).

In another terminal, install the pinned driver and run the
[example](websocket/index.mjs):

```bash
cd docs/api/bolt/examples/websocket
npm ci
node index.mjs
```

Set `RELATE_BOLT_PORT` to dial a non-default port (default `7687`).

The script wraps the global `WebSocket` to record the dialed URL and fails
unless a `ws://` URL was actually opened, proving the WebSocket channel —
not a raw TCP channel — carried the session. It then asserts
`RETURN 1 AS n` returns `1`, creates a node with a `$name` parameter and
reads it back, asserts a date, a datetime, a duration and cartesian and
geographic points hydrate as the JS driver's `neo4j.types.Date`,
`neo4j.types.DateTime`, `neo4j.types.Duration` and `neo4j.types.Point`
(never strings), asserts a write summary reports query type `w` with
`nodesCreated`, commits an explicit transaction and verifies the result is
visible afterwards, and prints `ok`.
