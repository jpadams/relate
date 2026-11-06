# Token budgets

Peak logical bytes are the query memory gate. They measure the query's
owned resident workspace. Query peak logical bytes are the high-water
mark of that workspace. The shared pool tracks the workspace the
statement currently holds, and the receipt reports the highest point
that workspace reached. `--concurrent-logical-workspace-limit` caps
that pool in logical bytes (`RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT`
when the flag is omitted). A positive limit turns admission on
(ADR-0452). Unset leaves it off. `0` is rejected. A query that outgrows the pool is refused
with a retryable 429. The refusal leaves the ledger untouched, and drop
releases the reservation.

The pool used to be described as peak tokens, where one token was 4096
workspace bytes, and receipts carried `TokenSchedule: v1` to name that
schedule. The unit is logical bytes. One quantum is
`CHARGE_QUANTUM_BYTES` (4096). The cap is logical bytes. RSS is not the
billed or controlled unit. A receipt reports the peak logical workspace
and carries no schedule name (ADR-0501).

Token admission replaces the removed `--max-active-queries` slot. It is
off unless `--concurrent-logical-workspace-limit` or
`RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT` is a positive size
(ADR-0452). There is no `--token-admission` switch. A set
`RELATE_TOKEN_ADMISSION` fails open. Off, overlapping queries
are not refused and responses carry no workspace receipt fields. On,
the pool capacity is that positive limit. The engine does not default
the pool. The concurrent logical-workspace pool
is the only executing-query gate. `--max-connections` stays the session
cap. There is no per-query execution budget and no per-query peak cap.

**Peak logical bytes** are the high-water mark of the owned resident
workspace. One quantum is 4096 logical bytes (`CHARGE_QUANTUM_BYTES`).
Any positive size rounds up to a whole quantum before it is reserved,
and a new admission reserves at least 4096 logical bytes (the admission
floor). The receipt stays at the high-water mark; a shrink does not
lower it. `TokenEstimate` and `ExecutionReceipt` keep those type names.
The field is `peak_logical_bytes`. They carry no schedule name (ADR-0501).

The workspace charges structures the statement allocates. A charge is
taken before the structure grows and released when that structure is
dropped, so sequential stages do not add their lifetimes together.

| Structure | Owned bytes |
| --- | --- |
| Sort or top-k buffer | The live row buffer and its key vector: vector headers and reserved capacity, one shared row schema per distinct schema, inline values at their slot size, a shallow node or relationship wrapper, and deep payload only for an allocation this statement created |
| Expand batch | The collector currently held, including each row's used-edge set |
| Grouped aggregation | The live group map and the input rows it still retains |
| Distinct set | The set's own entries. This covers a fold `count(DISTINCT …)`, `RETURN` / `WITH DISTINCT`, and `DISTINCT` inside a grouped aggregate. A duplicate offer does not grow the set |
| Create scratch | Nodes and relationships this statement created, until the scratch drops, plus the driving rows the write clause keeps. Each property map is a vector of entries fitted to the next power of two, and the redo log is charged for the slots that vector reserves. Property updates of those created entities change the same reservation. A string still owned by the committed graph is not billed again. Committed payload stays on the graph floor |

A row that only clones a graph-owned string, label vector, or property
map contributes its slot and wrapper. `Value::logical_bytes` remains the
deep payload function and is not applied to a borrowed value. Plain
streaming rows that no retaining structure holds stay uncharged. A copy
of an existing graph entity in the write scratch is not deep-charged.

`EXPLAIN` does not reserve and does not attach a `TokenEstimate`.
`PROFILE` reports the actual peak logical bytes on the plan as
`PeakLogicalBytes`, beside
`estimated_rows`, `db_hits`, `memory_bytes`, and `expression_built_bytes` (the
ADR-0463 per-allocation expression-build cap; its metric is
construction-work telemetry, not memory). `PROFILE` and a plain
run also carry the peak on `QueryResult.receipt` (`peak_logical_bytes`)
when admission is on.

## Admission

New work reserves either the admission floor (4096 logical bytes) or a
cached observed peak. It cannot spend the tail that running queries
still need. The tail is one eighth of
`--concurrent-logical-workspace-limit` (`tail = capacity / 8`).

- A new reservation succeeds only when `reserved + cost + tail <= capacity`.
- A running query may grow until `reserved + extra <= capacity`. The
  query already inside the pool can spend the tail and finish.
- The cost is the cached peak for this statement, or 4096 logical bytes
  when the cache has no cost. Charges round up to whole 4096-byte quanta
  before reserving. The reservation is taken with a compare-and-swap, so
  two retries cannot both pass on the same free logical bytes.
- That reservation stays until the live workspace passes it. After that
  the pool follows the live workspace, including below the original
  reservation. The receipt peak does not fall. Drop releases whatever
  is still reserved.

The cache is in-process and lost on restart. The key is the database
commit timestamp, the statement text with surrounding and repeated
whitespace collapsed, and a hash of the canonical parameter values.
Same text with a different parameter is a different entry. A commit
changes the timestamp and misses. The value is the maximum peak observed
for that key. A later refusal that got further raises it. A successful
receipt raises it to the real peak. A refusal at the door does not
write over a known cost. Entries live about 60 seconds, the expiry
refreshes when the entry is used or raised, and the map stays bounded.

## Plan cache

A retained plan is not a second customer limit. When admission is on,
the plan cache reserves the entry's measured logical bytes on this same
pool, under the admission ceiling, and only after the statement itself
has reserved workspace. The reservation stays until the entry is
evicted, replaced, or cleared. A hit does not reserve those bytes
again; it still reserves the statement's workspace, so a full pool
refuses the hit and leaves the entry in place. A miss that does not fit
is executed and not cached. A later miss inserts if the ceiling then
has room.

Statements that compile the original text — writes, `EXPLAIN` /
`PROFILE`, and rewritten compiles that fail — reserve that plan on the
statement for the run. The charge uses the admission ceiling and drops
with the statement. If it does not fit, the statement is refused with
`concurrent-logical-workspace-limit` and nothing is cached.

Admission off has no pool. The cache keeps its entry-count and
statement-text caps and does not reserve.

## Workflow

Admission runs after planning and before dispatch. There is no
in-engine queue.

| Step | Result |
| --- | --- |
| The shared pool cannot hold `cost` and still leave the tail | Retryable `concurrent-logical-workspace-limit`. The statement does not run. The cache is not lowered. |
| The owned workspace grows and rounds up to the next 4096-byte quantum | A pool that cannot hold the additional logical bytes is the same retryable limit. The failed growth is not kept. The cache rises to the high water already reached. |
| Success | The cache rises to the receipt peak. |
| Success, error, or cancel drops the ledger | Whatever is still reserved returns to the pool. |

```text
resource limit exceeded: concurrent-logical-workspace-limit limit=<n>
```

`limit=<n>` is the pool capacity in logical bytes. A workspace-limit
error carries `Error::receipt()` with the peak logical bytes counted
so far. A door refusal reports the cost that was refused. A successful
executed statement carries the same shape on `QueryResult.receipt` with
outcome `success`.
