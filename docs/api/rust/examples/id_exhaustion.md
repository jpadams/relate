# Exhausted graph IDs (ADR-0086)

Run `cargo run -p relate --example id_exhaustion --locked`. The
[compiled example](../../../../relate/relate/examples/id_exhaustion.rs)
checks that `Graph::create_node` and `Graph::create_rel` return typed errors
at `i64::MAX` without changing the graph, and that the `ExecGraph` write
view refuses the same allocations without adding redo.
It also checks typed `Graph::begin_write`/`begin_write_at` timestamp exhaustion
and `Graph::apply_redo` rejection of the persisted timestamp sentinel, while
`u64::MAX - 1` remains a valid explicit write/replay timestamp.
