# Query-type example

Run with `cargo run -p relate --example query_type`.

Every `QueryResult` carries its statement's query type
(`QueryResult.header.query_type`, ADR-0162): the Neo4j Query API v2 `queryType`
classification, derived from the logical plan before execution —
`QueryType::Read` (`"r"`) for a read-only statement, and a write splits on
its returned columns (ADR-0332): `QueryType::Write` (`"w"`) for a write
that returns no columns (e.g. `MATCH … SET` without RETURN), and
`QueryType::ReadWrite` (`"rw"`) for a write that returns columns (e.g.
`CREATE (n) RETURN n`). `QueryType::Schema` (`"s"`) is index/constraint
DDL. Plain runs, EXPLAIN
and PROFILE all carry it; EXPLAIN and PROFILE report the **wrapped**
statement's type, so a caller routes read-only vs read-write from `EXPLAIN`
without executing anything. The example runs one statement of each kind,
checks an `EXPLAIN CREATE` reports `w` while leaving the graph untouched,
and checks `PROFILE` reports the same type as `EXPLAIN`.
