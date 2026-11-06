# Offline adoption of a legacy default WAL

Run the [compiled Rust example](../../../../relate/relate/examples/adopt_legacy_wal.rs)
without arguments to copy a pre-ADR-0097 snapshot and WAL fixture into a
disposable directory, adopt it, reopen and check Ada and Bob's exact rows,
then reset it. The reopen migrates the legacy (value-encoding-1) store to
the current format in place — checkpoint plus WAL rotation before the first
query is served
([ADR-0127](../../../adr/0127-store-version-compatibility.md)):

```sh
cargo run -p relate --example adopt_legacy_wal
```

For an actual pre-ADR Native directory, first stop **both** possible database
owners. Independently verify that `<root>/wal` really belongs to `<root>` as
its default WAL; a closed independent database once opened with root == WAL
can have identical segment bytes. Back up the directory if appropriate, then
run the explicit offline assertion:

```sh
cargo run -p relate --example adopt_legacy_wal -- /path/to/database
```

The helper does not reset, replay, or move WAL/snapshot bytes. Reopen with
`Database::open` only after adoption completes successfully; if a sync fails,
inspect the `.cypherlite.lock` roles and retry after addressing the I/O error.
