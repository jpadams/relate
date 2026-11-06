//! Pattern predicates and path selectors: inline element `WHERE`s inside
//! pattern comprehensions, a named subpath beside the path variable, and
//! the scoping forms a subquery expression imports.

mod common;

use relate::Value;

use common::connection;

fn int(n: i64) -> Value {
    Value::Int(n)
}

#[test]
fn pattern_comprehension_inline_node_predicate_filters() {
    let mut conn = connection();
    conn.query(
        "CREATE (:A {prop: 1})-[:R]->(:B), (:A {prop: 2})-[:R]->(:B), \
         (:A {prop: 3})-[:R]->(:B)",
    )
    .unwrap();
    let result = conn
        .query("RETURN [(a:A WHERE a.prop > 1)-[r]-(b:B) | a.prop] AS result")
        .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![Value::List(vec![int(2), int(3)].into())]]
    );
}

#[test]
fn pattern_comprehension_predicate_reads_a_later_element() {
    let mut conn = connection();
    conn.query(
        "CREATE (:A {prop: 1})-[:R]->(:B {prop: 100}), (:A {prop: 2})-[:R]->(:B {prop: 200})",
    )
    .unwrap();
    let result = conn
        .query("RETURN [(a:A WHERE b.prop > 100)-[r]-(b:B) | [a.prop, b.prop]] AS result")
        .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![Value::List(
            vec![Value::List(vec![int(2), int(200)].into())].into()
        )]]
    );
}

#[test]
fn pattern_comprehension_relationship_predicate_filters() {
    let mut conn = connection();
    conn.query("CREATE (:A)-[:R {prop: 1}]->(:B), (:A)-[:R {prop: 2}]->(:B)")
        .unwrap();
    let result = conn
        .query("RETURN [(a:A)-[r:R WHERE r.prop > 1]-(b:B) | r.prop] AS result")
        .unwrap();
    assert_eq!(result.rows, vec![vec![Value::List(vec![int(2)].into())]]);
}

#[test]
fn path_and_named_subpath_bind_the_same_path() {
    let mut conn = connection();
    conn.query("CREATE (:A)-[:R]->()-[:R]->(:B)").unwrap();
    let result = conn
        .query("MATCH p = ANY SHORTEST (q = (:A)-->+(:B)) RETURN p = q AS result")
        .unwrap();
    assert_eq!(result.rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn expression_subquery_imports_an_unmentioned_call_scope_variable() {
    // The enclosing WITH projects only `p0`; `y` reaches the COLLECT body
    // solely through the inner naked CALL, which the static variable walk
    // does not enumerate. The projection must keep the whole incoming row.
    let mut conn = connection();
    conn.query("CREATE (:A), (:B)").unwrap();
    let result = conn
        .query(
            "MATCH (y) MATCH p0 = () \
             WITH p0, COLLECT { CALL { WITH y RETURN y AS y1 } RETURN y1 } AS c \
             RETURN size(c) AS n",
        )
        .unwrap();
    // The cross product has four rows; each COLLECT body yields exactly one
    // value, so every row's list has size one (the import is not dropped).
    assert_eq!(result.rows.len(), 4);
    assert!(result.rows.iter().all(|row| row == &vec![int(1)]));
}

#[test]
fn distinct_star_order_by_named_path() {
    let mut conn = connection();
    conn.query("CREATE (:A)-[:R]->(:B), (:B)-[:R]->(:A)")
        .unwrap();
    let result = conn
        .query("MATCH p = (a)-->(b) RETURN DISTINCT * ORDER BY p")
        .unwrap();
    assert_eq!(result.rows.len(), 2);
}
