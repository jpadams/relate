# Test tiers

[ADR-0251](../adr/0251-test-architecture.md) splits the suite into three
tiers chosen by what a test actually needs: **default** (no store at all),
**memory** (a `Database` on the `:memory:` VFS), and **disk** (real
filesystem behavior — WAL, checkpoint, path handling, fsync, file locking).
Tier membership is per test, not per crate; the disk tier is the only tier
allowed to create temporary directories.

This page records the per-file assignments as tests are re-tiered, so a
later audit can see why a file sits where it does without re-deriving it.

## Tier assignments

| File | Tier | Why |
| --- | --- | --- |
| `surfaces/relate-bolt/tests/concurrency.rs` | memory | The tests observe slot release, isolation, and timeouts over real sockets; the server's store location is incidental. `client_disconnect_mid_query_frees_the_slot_and_the_server_survives` passes `--uri :memory:` — a valid server URI — and still asserts write/read-back on the one server's store. |
| `surfaces/relate-load/tests/streaming.rs` | memory | All five tests assert pull-protocol behavior (statement batching, dedupe counts, typed-JSON headers, schema mapping, the `NonEmpty` refusal); the destination store location is incidental, so each opens `Database::open(":memory:")`. |
| `testing/relate-datasets/src/memory_shape.rs` (`#[cfg(test)]` probe tests) | disk | `probe_store` checkpoints and then measures the on-disk store directory (`store_directory_bytes` → `relate_storage::store_info`, which rejects `:memory:` — an in-memory store has no on-disk layout); `flat_fill_stops_when_the_limit_is_hit` also asserts the checkpointed directory is non-empty and contains the `wal` directory. |
| `testing/relate-datasets/tests/memory_shapes.rs` | memory, except `ring_replay_reports_the_same_logical_bytes` (disk) | The logical-byte pins never touch a path and open `Database::open(":memory:")`. The replay test's subject is WAL replay itself, and the engine appends to the WAL only on a durable VFS — a shared `MemoryVfs` re-open finds nothing to replay — so it keeps a real directory. |
