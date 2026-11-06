//! `serve` argument tests, split by policy (the LABS-392 parse/orchestration
//! split applied one level down): `unbound` owns the unbound-surface flag
//! policy and its table, `grammar` owns the parse grammar, once-only folds,
//! and value parsers. Each file stays well under the repo's 1k-line budget
//! as new serve knobs land.
//!
//! Both submodules reach the parse grammar through `crate::serve_args` and
//! the CLI's test argument builder through `crate::args`.

mod grammar;
#[cfg(any(feature = "http", feature = "mcp"))]
mod unbound;
