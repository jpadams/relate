# Example: fallible persistence JSON writes and reads, Arrow rows and WKB

Run the executable Rust documentation examples from the repository root:

```bash
cargo test --doc -p relate-model -p relate-arrow
```

- [`cypher_value_to_json`](../../../../relate/relate-model/src/serde_value.rs)
  writes an escaped user map containing a tagged Point and round-trips it via
  the v2 decoder; a Point with a non-finite coordinate fails with an
  `EncodeError` at `$.y`, or `$.place.y` through `props_to_json`. Its DateTime
  boundary doctest rejects UTC `NaiveDateTime::MAX` with `+01:00` before
  formatting, including the `$.payload.items[0]` property path and Serde
  error; it round-trips minute-aligned `+01:00`/`-01:00` values with canonical
  expanded-year RFC3339 strings at both local range limits.
- [`try_json_to_cypher_value_v2`](../../../../relate/relate-model/src/serde_value.rs)
  decodes a valid point, round-trips an ordinary map with a `__type` key,
  compares legacy and parameter map behavior (including a malformed nested
  legacy tag), and rejects a point missing `y`
  with a `DecodeError`. The example also checks that `CypherValue` Serde
  deserialization returns an error for malformed tagged JSON.
- [`parse_numeric_offset`](../../../../relate/relate-model/src/serde_value.rs)
  is a public model doctest run by `cargo test --doc -p relate-model`. It
  asserts that `+02:30` returns `Some(FixedOffset)` with 9,000 seconds, while
  `+00:60` and the multibyte-prefix input `é+02:30` return `None`. It also
  checks a valid negative offset. Persistence `Time` decoding uses this exact
  signed ASCII parser; Arrow timestamp metadata maps malformed signed offsets
  to `ArrowError` and handles `Z`/`UTC`/named zones separately.
- [`record_batch_to_rows`](../../../../surfaces/relate-arrow/src/from_arrow.rs)
  round-trips an explicitly marked graph value and an ordinary map through
  Arrow, then rejects an ambiguous legacy `json`-marked column. Its temporal
  doctest round-trips model JSON and Arrow `+02:30` zoned Time, shows unzoned
  Time64 and homogeneous native DateTime, then checks the per-row offset, local
  time, UTC instant and optional zone-name hint for heterogeneous DateTimes in
  single and chunked `typed-json-v1` batches. It also rejects a `+00:00:01`
  offset in model JSON/Serde and Arrow, including a DateTime after a fallback
  in a later chunk.
  Its lossless-column doctest round-trips a 3D Point's `z` alongside a 2D
  Geometry with a different per-cell SRID (single and chunked batches),
  preserves `2^53 + 1` and `i64::MAX` in an Int/Float typed-JSON column while
  an exact mix stays native Float64, and rejects external Infinity and a
  NaN-sentinel Point EMPTY in WKB (including an array-row error).
- [`infer_column_type`](../../../../surfaces/relate-arrow/src/schema.rs)
  infers the same `ColType::TypedJson` fallback for a Map and an incompatible
  `Int`/`Str` column; both use `typed-json-v1` `Utf8` fields when written.
- [`result_to_record_batch`](../../../../surfaces/relate-arrow/src/record_batch.rs)
  round-trips a uniform-SRID nested GeometryCollection with field SRID 4326,
  rejects a child SRID mismatch in a later chunk with its nested member path,
  accepts WKB depth 64 and rejects depth 65 before encoding. The native WKB
  writer and reader share one Arrow-internal depth limit, with the top-level
  geometry header counted as level 1.
- [`wkb_to_cypher_value`](../../../../surfaces/relate-arrow/src/geo_encoding.rs)
  decodes a big-endian point and a BinaryArray with a null cell; malformed
  WKB and a non-Binary array return errors.

The [JSON checkpoint example](../../../../relate/relate/examples/decoder_snapshot.rs)
checkpoints Point and Vector properties, adds a v2 `__type` map fixture, and
reopens valid v1/v2 snapshots. It then seeds valid Geometry tags directly and
inside a map/list property: both versions fail `Database::open` with
`Error::Storage` naming the node and nested property path, never a Null value.
It also edits a tagged Point coordinate to demonstrate malformed-tag failure.
In a separate temporary database it checkpoints valid Date, Time, DateTime and
Duration alongside Point/Vector, reopens and compares the values, then uses
typed Rust `Temporal`/`Value` parameters to trigger `Database::checkpoint`
errors for CivilDate/CivilDateTime, oversized Duration and invalid Time/DateTime
offsets (including nested list paths). It verifies the prior `graph.json` and
manifest bytes survive every failed write. The map is placed in the snapshot
fixture because Cypher queries do not allow Map-valued properties:

```bash
cargo run -p relate --example decoder_snapshot
```

CI builds and runs every `relate/relate/examples/*.rs` file. See the
[spec](../spec.md#persistence-json-and-query-parameters) for the v1/v2
`graph.json` compatibility policy.
