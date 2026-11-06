//! The resolved neighbor walk's committed + scratch arms (LABS-801) —
//! the unit-level coverage of `overlay/adjacency.rs`'s id-filtered walk,
//! beside the statement-level suites in `query_semantics_tests.rs`.

use crate::exec::test_support::write_scratch;
use crate::graph::Graph;

/// LABS-801: the resolved neighbor walk's write-overlay arm. The committed
/// side filters by interned id; the scratch's rel copies carry type
/// strings, so that arm compares the filter's names — a scratch rel of a
/// committed type and one of a type the store never interned both match
/// their filter, and a name nothing carries matches nothing.
#[test]
fn resolved_neighbor_walk_covers_committed_and_scratch_types() {
    use std::collections::BTreeMap;

    use relate_ir::Direction;

    use super::ExecGraph;
    use crate::store::RelTypes;

    let g = Graph::default();
    g.begin_write().unwrap();
    let a = g.create_node(vec!["A".into()], BTreeMap::new()).unwrap();
    let b = g.create_node(vec!["B".into()], BTreeMap::new()).unwrap();
    let committed = g.create_rel(a, b, "R".into(), BTreeMap::new()).unwrap();
    g.commit_write();

    let scratch = write_scratch(&g);
    let view = ExecGraph::write(&g, &scratch);
    // A scratch rel of the committed type (interned id known) and one of
    // a type the store never interned (the name arm).
    let scratch_r = view.create_rel(a, b, "R".into(), BTreeMap::new()).unwrap();
    let scratch_new = view
        .create_rel(a, b, "NEWTYPE".into(), BTreeMap::new())
        .unwrap();

    let walk = |types: &RelTypes| {
        let mut out = Vec::new();
        view.try_for_each_neighbor_resolved(a, Direction::Outgoing, types, |rid| {
            out.push(rid);
            Ok::<(), ()>(())
        })
        .unwrap();
        out
    };

    // One committed type: the committed rel and the scratch copy of it.
    let mut got = walk(&view.rel_types(&["R"]));
    got.sort();
    let mut want = vec![committed, scratch_r];
    want.sort();
    assert_eq!(got, want);
    // The never-interned type: the name arm admits the scratch rel.
    assert_eq!(walk(&view.rel_types(&["NEWTYPE"])), vec![scratch_new]);
    // Multiple types: both arms' matches, deduplicated.
    let mut got = walk(&view.rel_types(&["R", "NEWTYPE"]));
    got.sort();
    let mut want = vec![committed, scratch_r, scratch_new];
    want.sort();
    assert_eq!(got, want);
    // A name nothing carries matches nothing, scratch included.
    assert!(walk(&view.rel_types(&["GHOST"])).is_empty());
    // The untyped filter walks everything.
    assert_eq!(walk(&RelTypes::Any).len(), 3);
}
