# Example: rooted, bounded local CSV import

Run the [compiled Rust example](../../../../relate/relate/examples/load_csv.rs)
from the repository root:

```bash
cargo run -p relate --example load_csv
```

It creates a CSV directory, opens an in-memory database with
`OpenOptions { load_csv_dir: Some(root), load_csv_max_bytes: Some(1024), ..Default::default() }`,
imports two rows, and verifies that a 1025-byte source fails with the
generic byte-limit error. The configured directory is a separate capability;
the database path does not grant file access.
