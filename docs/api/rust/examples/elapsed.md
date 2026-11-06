# Engine-elapsed example

Run with `cargo run -p relate --example elapsed`.

Every `QueryResult` carries its statement's engine-reported wall-clock
elapsed (`QueryResult.header.elapsed`, ADR-0221): parse, plan, gates and dispatch
— execution, or EXPLAIN/PROFILE rendering — measured once around
`Connection::run_query`. It never includes row serialization or any
surface-side output, so it is the same boundary on every client surface
(the wire envelope's `elapsed_ms`, the C ABI's
`relate_result_elapsed_ms`, Python's `ResultSummary.elapsed_ms`, the
JS client's `elapsedMs` and the CLI's `.timer` line all re-source it).
The example runs a write, a read, an EXPLAIN and a PROFILE, and prints
each statement's elapsed in milliseconds.
