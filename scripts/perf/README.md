# Loadtest profiling on Linux

Profile scripts for the fraud/identity load tests. Run in order. All writes go to `/tmp`.

| Script | What it does | Needs |
| ------ | ------------ | ----- |
| `00-prereq.sh` | verify perf / toolchain / `perf_event_paranoid` | — |
| `00-prereq.sh --ebpf` | also verify bpftrace for CPU-level metrics | — |
| `01-build.sh` | build `fraud-loadtest`/`identity-loadtest` with debug symbols | cargo |
| `02-run-loadtest.sh [memory\|disk]` | `perf record` a run; writes `*.perf`, `*.out`, `*.report`, `*.folded` | perf |
| `03-bottleneck-summary.sh [mode]` | crate-layer + lock/wait share from a `.perf` | perf |
| `04-cpu-ebpf.sh [mode]` | `perf stat` IPC/cache/branch + optional bpftrace off-CPU/futex | perf, sudo, bpftrace(optional) |
| `05-flamegraph.sh [mode]` | render `/tmp/lt-<mode>.svg` | inferno or FlameGraph |

## Rules

- **Profile `memory`, not `disk`, for code-level bottlenecks.** A disk run is
  ~97% `fsync` (group commit works; ~26 commits/fsync at 50 workers). The
  engine's CPU path is invisible underneath.
- Build with `cargo build --profile profiling -p relate-datasets --bins` (`01-build.sh` does this; ADR-0244's symbolized-release profile, binaries in `target/profiling/`).
  Without symbols `perf` prints hex addresses.
- Knobs: `OPS`, `WARMUP`, `WORKERS`, `WRITE_FRACTION` env vars.

## Reading the summary

`03-bottleneck-summary.sh` prints self-time by function and an inclusive
share grouped by crate. For removal-first work, look for:

- a function with high **self** time that does pure data movement
  (materialization, cloning, a per-key interner lookup) — candidates to delete;
- `CommitWaiter` / futex high in **memory** mode — the single-writer
  serialize point (ADR-0014), a design boundary, not a removable layer.

The loadtest also emits commit-timing histograms to stderr with
`RELATE_COMMIT_TIMING=1` (parse+plan, overlay execute, wal_lock wait,
encode+write, `sync_all`, apply_redo), which is the ground truth for where
commit time goes on a disk run.
