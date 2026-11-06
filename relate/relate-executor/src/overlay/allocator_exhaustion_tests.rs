//! Allocator exhaustion at the id ceiling: a spent allocator errors
//! without wrapping and without enqueuing redo.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::{ExecGraph, WriteScratch};
use crate::graph::Graph;
use relate_core::Error;

#[test]
fn exhausted_allocator_never_wraps_or_enqueues_redo() {
    let graph = Graph::default();
    let node_alloc = Arc::new(AtomicU64::new(i64::MAX as u64 - 1));
    let rel_alloc = Arc::new(AtomicU64::new(i64::MAX as u64));
    let scratch = RefCell::new(WriteScratch::new(
        0,
        Arc::clone(&node_alloc),
        Arc::clone(&rel_alloc),
    ));
    let view = ExecGraph::write(&graph, &scratch);
    assert_eq!(
        view.create_node(vec![], BTreeMap::new()).unwrap(),
        i64::MAX - 1
    );
    assert!(matches!(
        view.create_node(vec![], BTreeMap::new()),
        Err(Error::Execution(_))
    ));
    assert!(matches!(
        view.create_rel(i64::MAX - 1, i64::MAX - 1, "R".into(), BTreeMap::new()),
        Err(Error::Execution(_))
    ));
    assert_eq!(node_alloc.load(Ordering::Relaxed), i64::MAX as u64);
    assert_eq!(rel_alloc.load(Ordering::Relaxed), i64::MAX as u64);
    assert_eq!(scratch.borrow_mut().take_redo().len(), 1);
}
