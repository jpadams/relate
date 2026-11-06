# Example: `--fetch-params` binds a fetched JSON document as a parameter

From the repository root, write a small JSON document and bind it in one
invocation — fetch → bind → `UNWIND` → `MERGE` without a wrapper script:

```bash
printf '{"items": [{"id": 1}, {"id": 2}]}' > /tmp/doc.json
cargo run -p relate-cli -- \
  --fetch-params 'doc=/tmp/doc.json' \
  --json :memory: \
  'UNWIND $doc.items AS i CREATE (:Item {id: i.id}) RETURN count(*)'
```

The flag is repeatable and composes with `--param`: assignments resolve in
argv order, and the last assignment for a name wins (an overridden
`--fetch-params` source is never fetched). A URL source works the same
way, e.g. `--fetch-params
'value=https://api.stackexchange.com/2.3/questions?tagged=neo4j&site=stackoverflow'`.

The e2e suite `e2e_fetch_params.rs` executes these shapes against the
binary (file and stub-HTTP URL sources, last-assignment-wins across
`--param`/`--fetch-params`, 64 MiB cap, exit-code split); the bundle suite
pins the fails-closed message for builds without the `fetch-params`
feature (ADR-0270).
