//! Shared payload allocation across returned values: returned copies of
//! one node share its property and label maps, and the graph's stubs for
//! the same label share theirs.

use std::sync::Arc;

use super::test_support::run_write;
use crate::exec::test_support::exec_autocommit;
use crate::graph::Graph;
use relate_core::Value;

#[test]
fn returned_copies_of_one_node_share_its_allocation() {
    let result = run_write("CREATE (n:Person {name: 'Ada'}) WITH n UNWIND [1, 2] AS i RETURN n");
    let Value::Node(first) = &result.rows[0][0] else {
        panic!("row 0: {:?}", result.rows);
    };
    let Value::Node(second) = &result.rows[1][0] else {
        panic!("row 1: {:?}", result.rows);
    };
    assert!(Arc::ptr_eq(&first.properties, &second.properties));
    assert!(Arc::ptr_eq(&first.labels, &second.labels));
}

#[test]
fn stubs_for_the_same_label_share_their_payloads() {
    let g = Graph::default();
    exec_autocommit(
        &g,
        "CREATE (:Person {name: 'Ada'})-[:KNOWS]->(:Person {name: 'Bea'})",
    )
    .unwrap();
    let ts = g.commit_ts();
    let ids = g.all_node_ids();
    assert_eq!(ids.len(), 2);
    let first = g.node_stub_at(ids[0], ts).unwrap();
    let second = g.node_stub_at(ids[1], ts).unwrap();
    assert!(
        Arc::ptr_eq(&first.labels, &second.labels),
        "one label vector serves every :Person stub"
    );
    assert!(
        Arc::ptr_eq(&first.properties, &second.properties),
        "stubs share the empty property map"
    );
    let rel = g.rel_stub_at(g.all_rel_ids()[0], ts).unwrap();
    assert!(Arc::ptr_eq(&first.properties, &rel.properties));
}
