# Example: ordinary parameter maps and typed constructors

From the repository root, run:

```bash
cargo run -p relate-cli -- \
  --param 'meta={"__type":"point","x":1}' \
  --param 'coords={"x":1.5,"y":2.5}' \
  --param 'iso="2024-03-14"' \
  --json :memory: \
  'RETURN $meta AS meta, point($coords) AS spatial, date($iso) AS day'
```

The JSON output has `meta` as `{"__type":"point","x":1}`, `spatial` as
`"point({srid: 7203, x: 1.5, y: 2.5})"`, and `day` as `"2024-03-14"`.
`__type` is map data, not an implicit constructor. The CLI integration test
`test_param_tagged_objects_are_ordinary_maps_and_typed_values_use_cypher_constructors`
executes this parameter shape against the binary.

Malformed or unrepresentable values exit 2 and print a parameter error to
stderr without running the query:

```bash
cargo run -p relate-cli -- --param 'meta={"x":}' :memory: 'RETURN 1'
```
