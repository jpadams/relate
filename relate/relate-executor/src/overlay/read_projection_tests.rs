//! Read-path projection over the overlay: ORDER BY reads the incoming
//! variable before the projection or the projected alias's own value,
//! and projected integers survive a variable-length return.

use super::test_support::run_write;
use crate::exec::test_support::{exec_autocommit, exec_cypher};
use crate::graph::Graph;
use relate_core::Value;

#[test]
fn order_by_uses_incoming_variable_before_project() {
    let r = run_write(
        "CREATE ({num: 1}), ({num: 3}), ({num: -5}) \
         WITH * MATCH (n) RETURN n.num AS prop ORDER BY n.num",
    );
    let nums: Vec<i64> = r.rows.iter().map(|row| row[0].as_int().unwrap()).collect();
    assert_eq!(nums, vec![-5, 1, 3]);
}

#[test]
fn variable_length_return_keeps_projected_integers() {
    let g = Graph::default();
    exec_autocommit(
        &g,
        "CREATE (:A {id: 1})-[:R]->(:A {id: 2})-[:R]->(:A {id: 3})",
    )
    .unwrap();
    let result = exec_cypher(
        &g,
        "MATCH (a:A)-[:R*1..2]->(b:A) RETURN a.id, b.id ORDER BY a.id, b.id",
    );
    let pairs: Vec<(i64, i64)> = result
        .rows
        .iter()
        .map(|row| {
            let Value::Int(left) = row[0] else {
                panic!("left cell is not an integer: {row:?}");
            };
            let Value::Int(right) = row[1] else {
                panic!("right cell is not an integer: {row:?}");
            };
            (left, right)
        })
        .collect();
    assert_eq!(pairs, vec![(1, 2), (1, 3), (2, 3)]);
}

#[test]
fn order_by_a_projected_alias_property_uses_that_value() {
    let g = Graph::default();
    exec_autocommit(&g, "CREATE (:A {k: 'b'}), (:A {k: 'a'})").unwrap();
    let result = exec_cypher(&g, "MATCH (n:A) RETURN {k: n.k} AS m ORDER BY m.k");
    let keys: Vec<&str> = result
        .rows
        .iter()
        .map(|row| {
            let Value::Map(map) = &row[0] else {
                panic!("expected a map: {row:?}");
            };
            let Value::String(k) = map.get("k").expect("k") else {
                panic!("expected a string key: {row:?}");
            };
            k.as_ref()
        })
        .collect();
    assert_eq!(keys, vec!["a", "b"]);
}
