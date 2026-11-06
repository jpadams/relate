# 0041. Time-triggered checkpoint when the WAL has work

- **Status**: Accepted
- **Date**: 2026-09-02
- **Deciders**: MemoryDB maintainers

## Context

ADR-0020 auto-checkpoints when the open WAL segment hits **64 MiB** or
**10 000 commits**. A long-lived process that writes slowly (REPL, Bolt,
an embedded handle) can keep a large uncheckpointed tail for hours.
Crash recovery then replays that tail. Operators asked for a clock so
a quiet-but-dirty WAL is still snapshotted.

ADR-0019 already rejected **timed batching of group commit** (sleep to
grow an fsync group). That is a different decision: this ADR does not
delay `sync_all`.

## Decision

Keep rotate / dump / recycle (ADR-0020). Add a **time** trigger next to
size and commit-count.

- Default interval is **300 seconds** (5 minutes).
- Interval `0` disables the timer. Size and commit-count still fire.
- On each tick, dump only if the WAL has work: open-segment `commits > 0`
  or `has_closed_segments()`. Idle processes do not write Parquet.
- `:memory:` does not start a timer (in-process WAL; last-handle drop
  already dumps).
- Config: `MEMORYDB_CHECKPOINT_INTERVAL_SECS`; Rust `OpenOptions`;
  CLI / Bolt `--checkpoint-interval-secs`. C ABI reads the env at
  `memorydb_open`.

The timer thread uses the same `checkpoint_busy` / `pin_rotate_and_dump`
path as a size-based kick. It does not hold `Arc<Shared>` across the
wait (Weak + stop `Condvar`) so close can wake it without a Drop
deadlock.

## Alternatives considered

- **Time only, drop size/count** — a busy writer would wait up to five
  minutes with a huge open segment. Rejected; those thresholds stay.
- **Dump on a fixed clock even when idle** — empty Parquet rewrite and
  WAL rotate with nothing to recycle. Rejected.
- **Sleep to grow a group commit** — already rejected in ADR-0019;
  this is not that.

## Consequences

- Persistent `Database::open` may spawn `memorydb-checkpoint-timer`.
- A later-references note on ADR-0020.
- Embedders that must not background-dump set interval `0`.
