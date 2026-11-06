# ADR-0368: One product binary: serve absorbs the bolt and mcp bins

Status: Accepted

Claim: decb658cbaabb0f999c0aebe15419492464c072d

## Context

Since ADR-0232, `cypherlite serve` hosts the HTTP Query API v2, the MCP
streamable-HTTP transport, and the Bolt server in-process on one open store,
one bind posture, and one shared credential. Two standalone binaries still
shipped beside it:

- `cypherlite-bolt` (ADR-0128): the full Bolt listener with its own flag
  vocabulary (`--bind`, `--uri`, TLS, the connection timeouts, the storage
  knobs), also the source of the scratch-image `entrypoint` bin;
- `cypherlite-mcp` (ADR-0232 §4): the MCP server over stdio or streamable
  HTTP with its own clap surface.

Three product binaries for one database is a maintenance and a trust-story
problem: three flag grammars, three startup banners, three
auth-token/identity resolutions, two places where "how do I serve this
store" has a different answer (LABS-381), and a Docker image that carries the
primary runtime plus a single-protocol alternative whose existence invites
"which one do I run?" The MCP case was the loudest: an MCP host that spawns
a child must spawn `cypherlite-mcp`, but every networked deployment wanted
`serve`, and only the standalone had the stdio transport — the one
transport MCP hosts actually use.

LABS-381 asks for one product binary. `cypherlite serve` is it.

## Decision

`cypherlite serve` is the one product binary for serving a CypherLite store
(LABS-381):

1. **Retire the standalone `cypherlite-bolt` and `cypherlite-mcp` bins.**
   Both crates stay exactly where they are — `surfaces/cypherlite-bolt`
   owns the Bolt protocol, transport, TLS, and PackStream;
   `surfaces/cypherlite-mcp` owns the MCP server and both its transports —
   and all their tests stay. `cargo build -p cypherlite-bolt` /
   `-p cypherlite-mcp` builds the libraries. The Docker `entrypoint` bin
   (ADR-0128) stays: the scratch image still needs its seed-then-exec
   PID-1 supervisor, and it execs `cypherlite serve` (ADR-0249).
2. **`serve --mcp` takes the stdio transport too**: `--mcp stdio` or
   `--mcp PORT`, a typed union (`McpMode::Stdio | McpMode::Http(u16)`),
   not a port-shaped string. `stdio` runs the MCP stdio transport in the
   foreground — newline-delimited JSON-RPC on the process's stdin/stdout
   until the host closes stdin, then the final checkpoint and a clean
   exit 0 — after every listener surface is spawned. Stdio skips the auth
   story entirely (ADR-0232 §1); an explicit `--auth` on a stdio-only run
   is a loud usage error, not an ignored flag, and no token file is
   persisted. The once-only policy rejects combining the two transports.
   `--mcp PORT` (streamable HTTP) is unchanged.
3. **The bolt e2e harness drives the crate's own `serve(&Args)` listener
   in-process** (a thread of the test process, the same flag vocabulary
   through `cli_args::Args`): the flags serve does not carry — TLS,
   `--no-websocket`, the handshake/idle/query timeouts — keep their
   coverage through the library entry point instead of a product process.
   The MCP crate's stdio e2e drives `serve_stdio` over a real duplex
   stream in-process, and the CLI's own e2e suite spawns
   `cypherlite serve --mcp stdio` as a child — the product path an MCP
   host actually spawns.
4. **The Docker image carries `cypherlite` and `entrypoint` only.** The
   image loses the TLS-capable `--entrypoint /bin/cypherlite-bolt`
   alternative: serve has no TLS, so an encrypted-Bolt deployment fronts
   the container with a TLS-terminating proxy.

Capabilities the standalone bolt binary uniquely exposed — TLS
(`--tls-cert`/`--tls-key`/`--tls-client-ca`), `--load-csv-dir`,
`--no-websocket`, `--ws-allow-origin`, `--allow-plaintext`, the
`--handshake/--idle/--query-timeout-secs` limits, `--max-result-bytes`,
and the storage knobs (`--memory-limit`, `--checkpoint-interval-secs`,
`--durability-mode`, `--wal-dir`) — have no product-binary surface after
this ADR. They remain library-level (embed `cypherlite_bolt::serve`) and
stay covered by the crates' own tests; giving serve any of them is a
future ADR, not a silent flag here.

## Alternatives considered

- **Keep the three binaries, document which is for what.** Rejected: three
  flag grammars and two auth/token stories keep drifting (they already
  differ in banner text, TLS, and limits), and users still face a
  "which binary" decision the product does not need to pose.
- **Retire the bins and extend `serve` with the bolt bin's full flag set
  (TLS, load-csv-dir, timeouts, storage knobs) in the same change.**
  Rejected for this ADR: that is a second, independent decision about
  serve's grammar (each flag needs its own feature-gate, help row, and
  tests per the CLI rules), and bundling it would make the retirement
  unreviewable. The gap is stated above so a follow-up ADR can close it
  deliberately.
- **Keep `cypherlite-bolt` as a test-only binary under another name.**
  Rejected: a test driver that is the old product server under a new name
  re-adds the third grammar and the "which one" confusion; the crate's own
  `serve(&Args)` entry point gives the e2e suite the same coverage
  in-process.
- **`--mcp stdio` as a transport spelled outside `--mcp` (e.g. a
  `serve --stdio` flag).** Rejected: `--mcp` already names the MCP
  surface; the transport is a value of that one flag (typed union), which
  also makes the once-only policy enforce "one MCP transport" for free.

## Consequences

- One product grammar for serving: `serve --http PORT --mcp PORT|stdio
  --bolt PORT` on one store, one banner, one credential story. The
  reference-style MCP host config becomes
  `{"command": "cypherlite", "args": ["serve", "--mcp", "stdio"]}`.
- The standalone-binary flag spellings (`--uri`, `--bind HOST:PORT`,
  `--identity` → serve's `--database`, `--host`/`--bolt PORT`,
  `--bolt-identity`) are documented as retired in the bolt spec's flag
  mapping; the ADR-0128 alias behavior survives only through the container
  `entrypoint`, which keeps translating them.
- Bolt TLS and server-side LOAD CSV are no longer reachable from any
  product binary (see the gap list above); documented as such in the bolt
  docs, and the smoke lane drops the two example runs that required them.
- The in-process bolt e2e harness can no longer assert on the server
  process's stderr (banner token, log lines, panic lines): the
  generated-token test reads the persisted `<store>.auth-token` file, and
  the log-line assertions are carried by the observable behavior (the
  closes/deadlines) instead. The harness also cannot isolate
  `CYPHERLITE_AUTH` per server, so the generated-token test requires the
  env to be unset (CI guarantees it).
- The memory-shape loadtest's bolt-binary case fails loudly until serve
  gains `--memory-limit`: its image and Makefile host path no longer
  build or copy the retired binary. The processing-memory loadtest runs
  on `cypherlite serve`.
- The Docker image name (`cypherlite-bolt:<version>`) is unchanged — a
  rename is a release/announcement concern, not part of this ADR.

References: LABS-381, ADR-0128, ADR-0232, ADR-0249, ADR-0293, ADR-0251,
ADR-0306.

## Later references

- ADR-0390 gives `cypherlite serve` the served store's `--memory-limit`
  (the query session's flag with its shared resolution) — the storage
  flag consequence item below anticipated it. The memory-shape
  loadtest's bolt cases spawn serve under it (LABS-424); the
  `--checkpoint-interval-secs` half for the processing-memory loadtest's
  bolt cases remains open.
- ADR-0431 adds a multi-artifact dimension to this one-binary posture:
  the release channel publishes per-platform extension tarballs and
  their signed channel index beside the one product binary, and
  capabilities install on demand into the XDG config dir instead of
  shipping as more binaries or image contents. The product grammar
  stays closed — the only new words are `run` (ADR-0429) and
  `extension` — and the Docker image still carries `cypherlite` and
  `entrypoint` only (LABS-469, LABS-552).
