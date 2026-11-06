# Clone round trip example

Run with `cargo run -p relate --example clone_roundtrip`.

Seeds a native directory database, clones it into a single-file `.rl`
bundle, and clones the bundle into a fresh directory — both directions are
the one `Database::clone` API, with each side's format inferred from the
`.rl` suffix on its path (ADR-0282) — and reads the seeded node back from
the cloned database. The source is opened read-only on both operations and
the destination keeps the source's snapshot codec.
