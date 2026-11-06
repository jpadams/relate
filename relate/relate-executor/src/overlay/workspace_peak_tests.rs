//! Workspace peak billing during a write: the driving rows stay billed
//! on the peak for the whole CREATE, above the scratch's own bytes.

use std::sync::Arc;

use crate::exec::test_support::{exec_autocommit, exec_write, write_scratch};
use crate::graph::Graph;
use crate::test_support::PeakPoolDouble;

#[test]
fn create_keeps_driving_rows_on_the_peak() {
    let g = Graph::default();
    exec_autocommit(&g, "UNWIND range(0, 127) AS i CREATE (:Cal {id: i})").unwrap();
    let pool = Arc::new(PeakPoolDouble::new(u64::MAX / 4));
    assert!(relate_core::tokens::PeakPool::try_grow(pool.as_ref(), 1));
    let mut ledger = relate_core::tokens::TokenLedger::new(u64::MAX / 4, 1, pool);
    let _guard = relate_core::tokens::LedgerGuard::install(&mut ledger);
    let scratch = write_scratch(&g);
    exec_write(
        &g,
        &scratch,
        "MATCH (s:Cal) CREATE (:CalCreate {id: s.id}) RETURN count(*)",
    )
    .unwrap();
    let s = scratch.borrow();
    let scratch_bytes = crate::workspace::scratch_created_bytes(
        &s.batch.nodes,
        &s.batch.rels,
        &s.batch.created_nodes,
        &s.batch.created_rels,
        &s.batch.out_adj,
        &s.batch.in_adj,
        s.redo.reserved_bytes(),
    );
    let (_, peak, _) = relate_core::tokens::workspace_meter().unwrap();
    // The driving MATCH vector stays billed for the whole CREATE, on top
    // of the created nodes. 128 stub rows are several KB past the scratch.
    assert!(
        peak > scratch_bytes + 4_096,
        "peak {peak} did not keep the driving rows above scratch {scratch_bytes}"
    );
}
