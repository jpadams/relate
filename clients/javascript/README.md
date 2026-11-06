# @relate/relate

Relate — an embedded, in-memory property graph engine with an openCypher
query front-end — for Node.js, via the C ABI.

```sh
npm install @relate/relate
```

## Usage

```js
const { Database } = require("@relate/relate");

const db = Database.open(":memory:");
const conn = db.connect();
const result = conn.query("RETURN 1 AS n");
// result.columns: string[]  // ["n"]
// result.rows: object[]     // [{ n: 1 }] — keyed by column name; cells are
//                           // the typed walker's JS values (ADR-0502):
//                           // int64 lossless (number | bigint), entities by
//                           // elementId, temporals as component classes
// result.stats: object      // the 11 mutation counters, zeros included
// result.bookmark: string | null  // "relate:<n>" after an auto-commit write
// result.notifications: object[] // engine-native query notifications
//                           // (ADR-0363); [] on a plain run, EXPLAIN's list
// result.single()           // the one row, or ResultNotSingleError
// result.consume()          // { counters, namespace, queryType, elapsedMs,
//                           //   notifications, bookmark }
// result.graph()            // { nodes, relationships } — dedup'd by elementId

// Typed parameters (the same classes results decode to), a FLOAT-typed 1,
// and full-range integers:
const { float, CypherDate, CypherDuration } = require("@relate/relate");
conn.query("RETURN $d AS d, $f AS f, $big AS big", {
  d: new CypherDate(2026, 11, 4),
  f: float(1),        // FLOAT 1.0, not the INTEGER 1
  big: 9007199254740993n, // exact beyond 2^53 (LABS-901)
});

// The Promise-returning form: errors ride the result handle back from the
// worker thread, as a typed RelateError (code, retryable, …).
await conn.queryAsync("RETURN 1/0").catch((e) => console.log(e.code));

// Or the tagged-template form (ADR-0222): interpolated values become
// auto-named $p0, $p1, … parameters — injection-safe by construction.
const name = "Alice";
conn.query`CREATE (p:Person {name: ${name}})`;
```

Full TypeScript types ship in the package (`index.d.ts`), and so does a
library skill for AI agents (`.agents/skills/relate/SKILL.md`;
`npx library-skills`).

The native library arrives as an optional dependency for your platform
(`@relate/<platform>-<arch>`); there are no install scripts.
For development against a repo checkout, set `RELATE_LIB` to the library
path or build `relate-ffi` so the loader finds it under `target/debug` or
`target/release`.

The full API contract — types, errors, lifecycle — lives in
[`docs/api/javascript/spec.md`](https://github.com/relatedb/relate/blob/main/docs/api/javascript/spec.md).

## Development (repo checkout)

```sh
cd clients/javascript
npm install
cargo build -p relate-ffi --release   # repo root; `make build` builds only the CLI
npm test                                  # node test.js && node loadtest/test.js
```

The loader (koffi) resolves the library in order: `RELATE_LIB` (any
path form), the installed platform package, then the repo's
`target/debug` and `target/release`. A stale pre-ABI-v2 library there
fails with an abiViolation error — rebuild the profile the loader picks.

The C header is `surfaces/relate-ffi/include/relate.h`; the
client requires `relate_abi_version() == 2`.

`make binding-js` from the repo root builds the library and packs the
client tarball into `target/` (never the source tree).

## Platform packages

Each OS/arch ships its own npm package, `@relate/<os>-<arch>`
with the native under `lib/` — no install scripts. Sources live in
[`platforms/`](./platforms) (one directory per key: `darwin-arm64`,
`darwin-x64`, `linux-arm64`, `linux-x64`, `win32-x64`); the release
workflow assembles them from the per-triple FFI builds
(`cargo build -p relate-ffi --release --target <triple>`), tars
cdylib + staticlib + header as `relate-ffi-<triple>.tar.gz`, and
packs one platform package per triple.

## License

Apache-2.0
