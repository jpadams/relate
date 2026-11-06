//! The overlay test modules' shared query runners: one fresh graph and
//! write scratch per statement.

use crate::exec::test_support::{exec_write, write_scratch};
use crate::graph::Graph;

fn run_write_result(q: &str) -> relate_core::Result<crate::QueryResult> {
    let g = Graph::default();
    exec_write(&g, &write_scratch(&g), q)
}

pub(super) fn run_write(q: &str) -> crate::QueryResult {
    run_write_result(q).unwrap()
}

pub(super) fn run_write_err(q: &str) -> String {
    match run_write_result(q) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected error for query: {q}"),
    }
}
