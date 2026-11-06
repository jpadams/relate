# Preflight and append a bounded WAL record

The same example runs as the `Wal::preflight_record` doctest in CI:

```sh
cargo test -p relate-storage --doc preflight_record
```

```rust
use relate_storage::Wal;

let mut wal = Wal::memory();
let ops = [];
let proof = wal.preflight_record(1, 1, &ops).unwrap();
assert_eq!(wal.lsn, 0); // preflight never appends
assert_eq!(wal.append_preflighted(proof).unwrap().lsn, 1);
wal.sync().unwrap();
assert_eq!(wal.replay(0).unwrap().len(), 1);
```

The proof owns its encoded payload, so the operations are free after
preflight. A physical write or barrier error has an uncertain outcome; discard
the handle and reopen rather than retrying it.
