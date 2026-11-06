//! The `complex-search-patterns` preview gate, ENABLED side (ADR-0371,
//! CIP-225 Rule 0): with `RELATE_PREVIEW=complex-search-patterns` a
//! SEARCH clause's MATCH statement keeps its own pattern shape — several
//! components, multi-hop and variable-length relationships, several bound
//! variables, predicates outside the binding — for a FULLTEXT source
//! exactly as for a VECTOR one (Neo4j's complex-pattern mode).
//!
//! `RELATE_PREVIEW` is process-global and read once per `Database::open`
//! (ADR-0371), so the enabled and closed sides are separate test binaries:
//! `complex_search_patterns_closed.rs` pins the Rule 0 rejections and must
//! never share a process with this file.

mod common;

use relate::Value;

use common::{connection, set_preview_once};

/// Two FULLTEXT indexes over the corpus's shape: node bios and relationship
/// notes, with the small graph the acceptance scenarios search.
fn search_store() -> relate::Connection {
    set_preview_once("complex-search-patterns");
    let mut c = connection();
    c.query(
        "CREATE FULLTEXT INDEX personBios IF NOT EXISTS \
         FOR (p:Person) ON EACH [p.bio]",
    )
    .unwrap();
    c.query(
        "CREATE FULLTEXT INDEX relBios IF NOT EXISTS \
         FOR ()-[r:KNOWS]->() ON EACH [r.note]",
    )
    .unwrap();
    c.query(
        "CREATE (alice:Person {name: 'Alice', bio: 'brave explorer'}), \
                (bob:Person {name: 'Bob', bio: 'quiet scholar'}), \
                (carol:Person {name: 'Carol', bio: 'bold inventor'}), \
                (alice)-[:KNOWS {note: 'brave trusted ally'}]->(bob), \
                (bob)-[:KNOWS {note: 'gentle mentor guide'}]->(carol)",
    )
    .unwrap();
    c
}

fn names(result: &relate::QueryResult) -> Vec<String> {
    result
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::String(name) => name.to_string(),
            other => panic!("expected a name string, got {other:?}"),
        })
        .collect()
}

#[test]
fn a_second_bound_variable_rides_along() {
    // FulltextSearchWithComplexPatternAcceptance "node full-text search
    // inside a connected pattern with a second bound variable".
    let mut c = search_store();
    let result = c
        .query(
            "MATCH (p:Person)-[:KNOWS]->(friend) \
             SEARCH p IN (FULLTEXT INDEX personBios FOR 'brave' LIMIT 5) \
             RETURN friend.name AS friend",
        )
        .unwrap();
    assert_eq!(names(&result), vec!["Bob".to_string()]);
}

#[test]
fn a_multi_hop_relationship_pattern_searches_its_own_edge() {
    let mut c = search_store();
    let result = c
        .query(
            "MATCH (x)-[r:KNOWS]->(y)-[r2:KNOWS]->(z) \
             SEARCH r IN (FULLTEXT INDEX relBios FOR 'trusted' LIMIT 5) \
             RETURN z.name AS z",
        )
        .unwrap();
    assert_eq!(names(&result), vec!["Carol".to_string()]);
}

#[test]
fn several_components_with_a_cross_component_predicate() {
    // FulltextSearchWithComplexPatternAcceptance "multiple components with
    // cross-component predicate": the second component's bound variable is
    // filtered by a predicate naming the first.
    let mut c = search_store();
    let result = c
        .query(
            "MATCH (seed:Person {name: 'Alice'}), (p:Person) \
             SEARCH p IN (FULLTEXT INDEX personBios FOR 'scholar' LIMIT 5) \
             WHERE seed.name <> p.name \
             RETURN p.name AS person",
        )
        .unwrap();
    assert_eq!(names(&result), vec!["Bob".to_string()]);
}

#[test]
fn a_variable_length_hop_beyond_the_search_binding() {
    let mut c = search_store();
    let result = c
        .query(
            "MATCH (p:Person)-[:KNOWS*1..2]->(target:Person {name: 'Carol'}) \
             SEARCH p IN (FULLTEXT INDEX personBios FOR 'brave' LIMIT 5) \
             RETURN p.name AS p",
        )
        .unwrap();
    assert_eq!(names(&result), vec!["Alice".to_string()]);
}

#[test]
fn the_plain_rule_0_shape_keeps_working() {
    // The opt-in widens acceptance; it never changes the single-node shape.
    let mut c = search_store();
    let result = c
        .query(
            "MATCH (p:Person) \
             SEARCH p IN (FULLTEXT INDEX personBios FOR 'brave' LIMIT 5) \
             RETURN p.name AS p",
        )
        .unwrap();
    assert_eq!(names(&result), vec!["Alice".to_string()]);
}
