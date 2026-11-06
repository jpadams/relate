# Go client — description

Package `relate` uses cgo to call `librelate_ffi`. It is the embed API for Go
programs. Sessions connect into one of the fixed namespaces (ADR-0117) —
`neo4j` (the default via `Connect`), `system` or `system_ontologies` — via
`ConnectNamespace`, and switch between transactions via `SetNamespace`.
Every `Result` also reports *its own* statement metadata (ADR-0285):
`result.Namespace` is the statement's resolved namespace — one of the typed
`Namespace` constants, the `USE` target
when the statement carries one, else the session namespace — and
`result.QueryType` is the plan-time classification — one of the typed
`QueryType` constants (`QueryTypeRead`, `"r"`; `QueryTypeWrite`, `"w"`;
`QueryTypeReadWrite`, `"rw"`; or `QueryTypeSchema`, `"s"`) — so a caller can
tell reads from writes and learn which namespace a
`USE`-routed statement actually ran in.
`result.Notifications` carries the statement's query notifications
(ADR-0363) in the engine-native shape: an empty slice on a plain run, and
typed entries from `EXPLAIN`/`PROFILE` with the notification's code, GQLSTATUS,
severity, category, title, description, remedy, and — on operator-scoped
kinds — the raising operator's id.
Cells arrive as typed Go values (`int64`, `string`, `relate.Node`,
`relate.Date`, …) decoded through the C ABI's value walker, and parameters
are bound through its typed builder, so integers stay lossless and no JSON
crosses the boundary. A failed statement is a `*relate.Error` with the
engine's `Code`, `GQLStatus`, `Stage` and `Retryable`, so a retry loop branches
on the typed flag rather than on message text.
Examples: [`examples/params_and_values.go`](examples/params_and_values.go)
(typed parameters and cells) and [`examples/errors.go`](examples/errors.go)
(typed errors).
`cmd/fraud-loadtest` and `cmd/identity-loadtest` run the dataset
load-test mix through this client ([`docs/datasets.md`](../../datasets.md)).
