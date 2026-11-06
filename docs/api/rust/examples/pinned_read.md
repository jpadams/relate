# Pinned read example

Run with `cargo run -p relate --example pinned_read`.

The caller keeps the graph pin alive and captures `Graph::schema_snapshot()`
at the same admission boundary, before the subsequent commit. The MATCH and
SHOW INDEXES queries both use the original view. The example also constructs
`ExecGraph::read_at_with_schema` and `ExecGraph::write_with_schema`; the latter
sees its own uncommitted index drop without changing the admitted or live view.
It also applies `CatalogDerivedConstraint` redo with a named constraint and
queries the resulting `enforcedLabel` column on a newly admitted view.
