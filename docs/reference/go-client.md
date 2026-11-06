# Go Client

The Go client (`clients/go`, module
`github.com/relatedb/relate/clients/go`) binds the product **C ABI**
through cgo.

## Link it

```bash
cargo build -p relate-ffi
```

Point the linker and loader at the FFI library:

```bash
export CGO_LDFLAGS="-L$PWD/target/debug -lrelate_ffi"
export DYLD_LIBRARY_PATH=$PWD/target/debug   # LD_LIBRARY_PATH on Linux
```

On macOS the linker needs `librelate_ffi.dylib` — a Linux `.so` left in
`target/debug` fails with `unknown file type`.

## Open, query, close

```go
import relate "github.com/relatedb/relate/clients/go"

db, err := relate.Open(":memory:")       // or a directory / .rl bundle
conn, err := db.Connect()
defer conn.Close()                           // implicit rollback

result, err := conn.Query("RETURN 1 AS n")
result, err = conn.QueryParams("RETURN $n AS n", map[string]any{"n": 1})
// result.Columns []string
// result.Rows    [][]any
```

`Query` is `QueryParams` with a nil map; `QueryParams` sends `json_params` to
`relate_query_params`.

## Transactions

`Begin` / `Commit` / `Rollback` are an explicit transaction (ADR-0028). `Close`
is an implicit rollback.

## Errors

Query errors include the ABI message, e.g. `write-write conflict` — retry in the
caller (ADR-0017).

## Load-test binaries

`cmd/fraud-loadtest` and `cmd/identity-loadtest` wrap this client. Flags and
report JSON are in [Dataset suites](https://github.com/relatedb/relate/blob/main/docs/datasets.md).

Full contract: [`docs/api/go/spec.md`](https://github.com/relatedb/relate/blob/main/docs/api/go/spec.md).

## See also

- [C FFI](c-ffi.md) — the ABI this client wraps.
- [Java](java-client.md) and [JavaScript](javascript-client.md) — the same ABI in other languages.
