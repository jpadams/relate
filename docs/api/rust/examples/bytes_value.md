# Byte-array example

Run with `cargo run -p relate --example bytes_value`.

`Value::Bytes(Arc<[u8]>)` is the engine's statement-scoped byte array
(ADR-0327). The example passes `Value::Bytes([0x00, 0x01, 0xFF])` as `$b`
through `query_with_params` and checks it comes back unchanged, also inside
a list, and that the column type is `BINARY`. It then tries
`CREATE (:Blob {b: $b})`, checks the statement fails with
`Error::TypeError` (byte arrays cannot be stored as properties), and checks
that no `:Blob` node was written.
