# Bolt Server

`relate serve --bolt PORT` runs the embedded engine behind a **Bolt**
transport, so the
official Neo4j drivers can connect and run openCypher. Bolt is a transport, not
Neo4j Cypher parity: the language contract stays the
[openCypher TCK](../limitations.md). Neo4j-specific clauses fail as
syntax or semantic errors.

## Getting the binary

The standalone `relate-bolt` binary is **retired** (ADR-0368): Bolt is
served by the one product binary, the `relate` CLI's `serve` command —
the CLI ships in `install.sh` and the release assets, and the
`relate-bolt` crate stays as the protocol/transport library behind
serve's Bolt surface.

## Run it

```bash
relate serve --host 127.0.0.1 --bolt 7687 --database :memory:
relate serve --bolt 7687 --database ./graph --auth none
relate serve --help
```

From a source checkout without installing,
`cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687
--database :memory:` is equivalent. The old standalone spellings map onto
serve: `--uri PATH` → `--database PATH`; `--bind HOST:PORT` → `--host HOST
--bolt PORT`; `--identity` → `--bolt-identity`; `--auth`, `--read-only`,
`--max-connections` keeps its name. `--max-active-queries` is removed;
admission is a positive `--concurrent-logical-workspace-limit`
(pool capacity in logical bytes; ADR-0452). Omit it and admission stays off.

The storage knobs the standalone binary carried — `--wal-dir`,
`--checkpoint-interval-secs`, `--durability-mode` — are
**library-level only** since ADR-0368 (embed `relate_bolt::serve`;
their `RELATE_*` env fallbacks still pass through the container).
`--logical-graph-size-limit` is a `relate serve` flag (ADR-0390).
Crate-level knobs:

| Knob | Default | Meaning |
|------|---------|---------|
| `--wal-dir <PATH>` | unset | Local WAL directory (ignored for `:memory:`) |
| `--checkpoint-interval-secs <N>` | `300` (`RELATE_CHECKPOINT_INTERVAL_SECS`) | Seconds between timed checkpoints when the WAL has work. `0` disables. Persistent URIs only. |
| `--durability-mode <MODE>` | `full` | `full`, `everysecond`, or `snapshot` WAL durability |
| `--logical-graph-size-limit <SIZE>` | unset (`RELATE_LOGICAL_GRAPH_SIZE_LIMIT`; `0`/omit = unlimited) | Live graph + index cap in logical bytes. `SIZE` is bytes, or an integer plus `mb`/`mib`/`gb`/`gib`. Exceeding it fails a growing write with `capacity exceeded: used=… limit=…`. |

`full` acknowledges only after the WAL barrier. `everysecond` acknowledges
after apply and can lose the latest approximately one second of acknowledged
writes in a crash (or more if the barrier is delayed). `snapshot` can lose all
writes since the latest completed checkpoint. For `snapshot`, tune the normal
loss window with `--checkpoint-interval-secs` (default 300); `0` disables only
timed checkpoints, not threshold, explicit, or clean-close checkpoints.

## Connect from a driver

The server speaks **Bolt 4.4 and Bolt 5.4** (handshake magic `0x6060B017`)
with PackStream v1. A client's proposals must cover one of them — exactly
or inside a version range — and the first covering proposal in client
order wins. If none does, the handshake gets the reply `00 00 00 00` and
the connection closes (ADR-0303, ADR-0484).
HELLO must carry a `user_agent` (every official driver sends one); it
answers `server: "Relate/<version>"`.

```python
# pip install neo4j  (driver v6)
from neo4j import GraphDatabase

with GraphDatabase.driver("bolt://127.0.0.1:7687", auth=None) as driver:
    with driver.session() as session:
        print(session.run("RETURN 1 AS n").single()["n"])
```

`RETURN 1 AS n` is the smoke test the server's contract tests run with the
official Python driver.

## Protocol surface

| Message | Support |
|---------|---------|
| `HELLO` | Extra `user_agent` (required; a HELLO without one is rejected), auth token per `--auth` at 4.4; at 5.4 a `bolt_agent` map is required too and the auth moves to LOGON (ADR-0484). SUCCESS carries `hints: {"connection.recv_timeout_seconds": 120}`; the server sends NOOP keep-alives during long requests |
| `RUN` | `RUN cypher [params]`, then `PULL` / `DISCARD`. SUCCESS carries `fields`, `t_first`, and `qid` inside a transaction |
| `PULL` | RECORD(s), then `{has_more: true}` while rows remain, else the summary: `type`, `t_last`, `db`, non-zero `stats`, and a `bookmark` after an auto-commit statement |
| `DISCARD` | Drops rows; same summaries as `PULL` |
| `RESET` | Clears failed state; rolls back an open transaction; interrupts work pipelined ahead of it (IGNORED) |
| `GOODBYE` | Close |
| `BEGIN` | Starts an explicit transaction on the session connection. Each `RUN` in it keeps its own result, addressed by `qid` |
| `COMMIT` | One WAL commit of that transaction; SUCCESS carries a `bookmark` |
| `ROLLBACK` | Discards uncommitted writes |
| `ROUTE` | Single-server routing table; the home database is `neo4j` |
| `LOGON` | 5.4 only: the auth token (`basic`/`none`) — the check HELLO carried at 4.4; success opens the session |
| `LOGOFF` | 5.4 only: SUCCESS and back to the AUTHENTICATION state; a fresh LOGON reopens the session |
| `TELEMETRY` | 5.4 only: answered SUCCESS `{}` — the server collects nothing (the HELLO hint `telemetry.enabled: false` says so) |

`COMMIT` or `ROLLBACK` without `BEGIN`, a nested `BEGIN`, and `PULL` or
`DISCARD` with nothing to pull are protocol breaches: one
`Neo.ClientError.Request.Invalid` FAILURE, then the connection closes. A
runtime error (for example `1 / 0`) is reported at `PULL`, as with Neo4j;
syntax and planning errors fail the `RUN`.

Not supported: reactive, the Bolt 5.7+ manifest handshake
(`00 00 01 FF`; decided against, ADR-0484 — see the
[compatibility matrix](https://github.com/relatedb/relate/blob/main/docs/api/bolt/compatibility.md)).

A write-write conflict is a FAILURE with message `write-write conflict`
(reported at `PULL`); the server does not retry — retry in the caller
(ADR-0017).

Full contract: [`docs/api/bolt/spec.md`](https://github.com/relatedb/relate/blob/main/docs/api/bolt/spec.md).

## See also

- [C FFI](c-ffi.md) — the in-process multi-language boundary (Bolt is the remote one).
- [Limitations](../limitations.md) — one engine, no clustering, no auth.
