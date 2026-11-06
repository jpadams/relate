# Rust crate — staged runs and heartbeats

Part of the [Rust spec](spec.md). Two `Connection` entry points and three
types serve network transports — the Bolt server is the in-tree user — that
report planning failures and execution failures at different points and
must keep their client connection alive during long work.

## `Connection` methods

| Method | Contract |
| --- | --- |
| `query_staged(cypher, params, token, heartbeat)` | `Result<Executed>`: run a statement and report which phase a failure came from (ADR-0319). `Err` is a failure **before** execution — parse, semantic validation, planning, the compile-time gates, a missing parameter (ADR-0308). `Ok(Executed::Rows(QueryResult))` is the finished result; `Ok(Executed::Failed(ExecutionFailure))` is a failure while executing the planned statement (a runtime error such as `/ by zero`, a constraint violation, a write-write conflict, capacity, cancellation). `ExecutionFailure` carries the planned result `header` (`ResultHeader`: the output `columns`, `column_types` — `ANY` per column, ADR-0224's no-rows stamp — `query_type`, `namespace`, `elapsed`) and the `error`; a planned statement's output names always derive (the UNION column-agreement check ran in `plan`), so a dispatch-phase failure is always staged, never `Err`. `token` cancels as in `query_with_params_cancellable`. `heartbeat: Option<Heartbeat>` ticks on the calling thread while the statement runs on the query worker (ADR-0312) — on a caller-stack-asserted connection ([`Connection::assume_worker_stack_on_caller_thread`](spec.md), ADR-0504) a bounded read runs inline and has no wait, so its heartbeat is never due, while an escalating statement keeps the worker wait and ticks exactly as on an unasserted connection. An execution failure inside an explicit transaction fails the transaction, like any query error. `Executed::into_result()` gives the `query*` shape (`Failed` becomes its error). |
| `commit_with_heartbeat(heartbeat)` | `commit()`, ticking `heartbeat` on the calling thread while the flush runs (ADR-0312) — including its return: the committed transaction's bookmark (ADR-0502 §7), `None` when the transaction committed nothing. One shape for every connection (ADR-0504): the commit prepares on the connection thread — preconditions first (a failed transaction errors before anything else runs) — and routes the flush on the drained redo, the commit's own routing fact. An empty drain is the no-op, flushed inline with nothing waiting; a redo drain flushes on the connection's worker (the running one, or one spawned for the flush), keeping the heartbeat's wait shape. A worker-spawn failure after the drain never drops the prepared work — the flush completes inline, data safety over the one lost wait shape. `commit()` is the no-heartbeat path. |

## Types

| Type | Contract |
| --- | --- |
| `Heartbeat<'a>` | A callback the caller runs on its own thread once per elapsed interval while a statement or a COMMIT executes on the connection's query worker (ADR-0312, ADR-0259). Built with `Heartbeat::every(interval, tick) -> Result<Heartbeat>`: the fields are private and a zero `interval` fails with `SemanticError: heartbeat interval must be non-zero` (ADR-0334) — a zero interval would tick without ever waiting. Work that finishes inside the interval never ticks and arms no timer; on a caller-stack-asserted connection (ADR-0504) bounded-work statements run inline — no wait exists — so only the escalating statements can ever tick. The Bolt server writes NOOP keep-alive chunks from it. |
| `Executed` / `ExecutionFailure` | The staged outcome of `Connection::query_staged` (ADR-0319): `Executed::Rows(QueryResult)` or `Executed::Failed(ExecutionFailure { header: ResultHeader, error })`, where `ResultHeader` is the metadata `QueryResult` also carries (ADR-0343). |

## Example

```rust
use std::collections::BTreeMap;
use std::time::Duration;
use relate::{CancelToken, Database, Executed, Heartbeat};

let db = Database::open(":memory:").unwrap();
let mut conn = db.connect().unwrap();

// A runtime error is staged with the planned columns; a syntax error is Err.
let staged = conn
    .query_staged("UNWIND [1, 0] AS x RETURN 10 / x AS v", BTreeMap::new(), CancelToken::new(), None)
    .unwrap();
let Executed::Failed(failure) = staged else { panic!("10 / 0 fails while executing") };
assert_eq!(failure.header.columns, ["v"]);
assert!(conn.query_staged("RETURN", BTreeMap::new(), CancelToken::new(), None).is_err());

// A heartbeat ticks on this thread only while the work outlives its interval.
let mut ticks = 0;
let mut tick = || ticks += 1;
conn.begin().unwrap();
conn.query("CREATE (:N)").unwrap();
let heartbeat = Heartbeat::every(Duration::from_secs(60), &mut tick).unwrap();
let bookmark = conn.commit_with_heartbeat(heartbeat).unwrap();
assert!(bookmark.is_some());
assert_eq!(ticks, 0);
```

The same code runs as the doctests of `Connection::query_staged` and
`Connection::commit_with_heartbeat`
(`relate/relate/src/connection/staged.rs`).
