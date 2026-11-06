# JavaScript Client

The JavaScript client (`clients/javascript`, npm package
`@relate/relate`) wraps the product **C ABI** (the typed ABI v2,
ADR-0502) — it does not invent a parallel protocol. Requires Node 18+.

## Install and open

```bash
npm install @relate/relate
```

The native library normally arrives via the platform optional-dependency for
the current os/arch (`@relate/<platform>-<arch>`, ADR-0278) —
no install scripts. In a repo checkout, or on a platform without a published
package, it is found via `RELATE_LIB` or the repo `target/{debug,release}`
directories (the dev fallback). The client checks
`relate_abi_version() === 2` at load and refuses anything else.

```js
const { Database } = require("@relate/relate");

const db = Database.open(":memory:");   // or a directory / .rl bundle
const conn = db.connect();

const result = conn.query("MATCH (n:Person) RETURN n.name AS name, n.age AS age");
// result.rows:    objects keyed by column name; cells are the typed value
//                 walker's JS shapes (ADR-0502) — int64 lossless
//                 (number | bigint), entities by elementId
```

## API

| Call | Returns | Notes |
|------|---------|-------|
| `Database.open(uri)` | `Database` | `:memory:`, a directory, or a `.rl` bundle |
| `db.connect()` | `Connection` | One connection per call |
| `conn.query(cypher, params?)` | `Result` | `params` lowers into the typed params builder |
| `conn.queryAsync(cypher, params?)` | `Promise<Result>` | The worker-thread form; errors ride the result handle back |
| `conn.begin()` / `rollback()` | — | Explicit transaction; auto-commit otherwise |
| `conn.commit()` | `string | null` | The transaction's bookmark, or null when it committed nothing |

Failing calls throw `RelateError` with the engine's classification
(`code`, `gqlStatus`, `stage`, `retryable`); retry only `retryable` errors
(the write-write conflict is one).

## Load-test CLIs

`bin/fraud-loadtest.js` and `bin/identity-loadtest.js` wrap this client. Flags,
workload mix, and report JSON are in [Dataset suites](https://github.com/relatedb/relate/blob/main/docs/datasets.md).

Full contract: [`docs/api/javascript/spec.md`](https://github.com/relatedb/relate/blob/main/docs/api/javascript/spec.md).

## See also

- [C FFI](c-ffi.md) — the ABI this client wraps.
- [Go](go-client.md) and [Java](java-client.md) — the same ABI in other languages.
