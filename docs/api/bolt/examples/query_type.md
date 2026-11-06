# Example: read the query type over Bolt

The SUCCESS that ends every result (the last PULL, or DISCARD) carries a
`type` key with the statement's Neo4j Query API v2 `queryType` code
(ADR-0162, ADR-0309, the ADR-0332 split): `"r"` (read), `"w"` (a write with
no returned columns), `"rw"` (a write with returned columns) or `"s"`
(schema DDL), beside the statement's non-zero `stats` counters.
Neo4j drivers expose them as `summary.query_type` and `summary.counters`.
The engine classifies from the plan before execution, so `EXPLAIN` and
`PROFILE` report the wrapped statement's type.

In one terminal, run from the repository root:

```bash
cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687 --database :memory:
```

In another terminal with the official Neo4j Python driver installed
(`python3 -m pip install neo4j`), run the [driver example](query_type.py)
from the repository root:

```bash
python3 docs/api/bolt/examples/query_type.py
```

The example runs one statement of each kind and asserts the driver-visible
`query_type`: a read (`r`), a write with returned columns (`rw`), writes with
no returned columns (`w`: a bare `CREATE`, a `MATCH … SET` and a
`MATCH … DETACH DELETE`), an index-DDL statement (`s`), and an `EXPLAIN` of
a write reporting `w` while executing nothing. It also checks
`summary.counters` for the writes (one node and one label added; one node
deleted) and the index (one index added). CI runs this same script in the
`relate-bolt` job.
