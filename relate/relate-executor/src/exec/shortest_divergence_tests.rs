//! Expression form versus `MATCH` clause over several start candidates
//! (ADR-0189). The clause answers one shortest trail (or all of them) per
//! (start, end) pair; the expression form answers one value per input row,
//! so with `min_hops` <= 1 it takes the first start candidate that has a
//! trail. These tests pin where the two diverge and where they agree, and
//! that the divergence depends on candidate order only.

use relate_core::Value;

use crate::graph::Graph;

use super::test_support::{exec_cypher, Builder};

/// Starts in creation order `starts`, ends t1 and t2.
/// s1 -> t1 takes 3 hops, s2 -> t1 takes 1, s2 -> t2 takes 2.
fn graph(starts: [&str; 2]) -> Graph {
    let b = Builder::new();
    let s: Vec<_> = starts.iter().map(|id| (*id, b.node(id, &["S"]))).collect();
    let t1 = b.node("t1", &["T"]);
    let t2 = b.node("t2", &["T"]);
    let mid = |id: &str| b.node(id, &[]);
    let at = |name: &str| s.iter().find(|(id, _)| *id == name).unwrap().1;
    b.chain(&[at("s1"), mid("a"), mid("b"), t1]);
    b.edge(at("s2"), t1);
    b.chain(&[at("s2"), mid("c"), t2]);
    b.done()
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Null => "null".into(),
        other => panic!("unexpected {other:?}"),
    }
}

/// The clause's (start, end, length) rows for `selector` over `pattern`,
/// sorted.
fn clause(g: &Graph, selector: &str, pattern: &str) -> Vec<String> {
    let q = format!(
        "MATCH p = {selector}({pattern}) \
         RETURN startNode(p).id, endNode(p).id, length(p)"
    );
    let mut rows: Vec<String> = exec_cypher(g, &q)
        .rows
        .iter()
        .map(|r| r.iter().map(text).collect::<Vec<_>>().join(">"))
        .collect();
    rows.sort();
    rows
}

/// The expression form's (start, end, length) for `selector`, or one
/// `null` cell: always one row.
fn expression(g: &Graph, selector: &str, pattern: &str) -> Vec<String> {
    let q = if selector == "shortestPath" {
        format!(
            "WITH {selector}({pattern}) AS p \
             RETURN startNode(p).id, endNode(p).id, length(p)"
        )
    } else {
        format!(
            "UNWIND coalesce({selector}({pattern}), [null]) AS p \
             RETURN startNode(p).id, endNode(p).id, length(p)"
        )
    };
    let mut rows: Vec<String> = exec_cypher(g, &q)
        .rows
        .iter()
        .map(|r| r.iter().map(text).collect::<Vec<_>>().join(">"))
        .collect();
    rows.sort();
    rows
}

const OPEN: &str = "(:S)-[*]->(:T)";

#[test]
fn single_path_takes_first_candidate_with_a_trail_not_the_shortest() {
    let g = graph(["s1", "s2"]);
    assert_eq!(
        clause(&g, "shortestPath", OPEN),
        ["s1>t1>3", "s2>t1>1", "s2>t2>2"]
    );
    // One row, from s1, the first candidate: 3 hops, though s2 has 1.
    assert_eq!(expression(&g, "shortestPath", OPEN), ["s1>t1>3"]);
}

#[test]
fn single_path_result_follows_candidate_order() {
    let g = graph(["s2", "s1"]);
    // s2 now comes first; its candidate-local pick (the shallowest end
    // layer) is the 1-hop trail to t1.
    assert_eq!(expression(&g, "shortestPath", OPEN), ["s2>t1>1"]);
}

#[test]
fn all_paths_first_candidate_sets_the_length() {
    let g = graph(["s1", "s2"]);
    assert_eq!(
        clause(&g, "allShortestPaths", OPEN),
        ["s1>t1>3", "s2>t1>1", "s2>t2>2"]
    );
    // s1 sets length 3; s2's shorter trails are dropped.
    assert_eq!(expression(&g, "allShortestPaths", OPEN), ["s1>t1>3"]);
    let g = graph(["s2", "s1"]);
    // s2 sets length 1; s1's 3-hop trail is dropped.
    assert_eq!(expression(&g, "allShortestPaths", OPEN), ["s2>t1>1"]);
}

#[test]
fn bound_end_expression_is_first_candidate_clause_is_per_start() {
    let g = graph(["s1", "s2"]);
    let end = "(:S)-[*]->(:T {id: 't1'})";
    assert_eq!(clause(&g, "shortestPath", end), ["s1>t1>3", "s2>t1>1"]);
    assert_eq!(expression(&g, "shortestPath", end), ["s1>t1>3"]);
    assert_eq!(expression(&g, "allShortestPaths", end), ["s1>t1>3"]);
}

#[test]
fn bound_start_and_end_agree() {
    // One start candidate: nothing to combine, so the forms agree.
    let g = graph(["s1", "s2"]);
    for sel in ["shortestPath", "allShortestPaths"] {
        let pat = "(:S {id: 's2'})-[*]->(:T {id: 't1'})";
        assert_eq!(clause(&g, sel, pat), expression(&g, sel, pat), "{sel}");
    }
}

#[test]
fn min_hops_above_one_takes_shortest_over_all_candidates() {
    let g = graph(["s1", "s2"]);
    // Enumeration: the global minimum of 2 hops (s2 -> c -> t2). The
    // clause answers per pair: t1 from s2 needs a 2+ trail, which does
    // not exist, so only s1 -> t1 (3) and s2 -> t2 (2) remain.
    let pat = "(:S)-[*2..]->(:T)";
    assert_eq!(clause(&g, "shortestPath", pat), ["s1>t1>3", "s2>t2>2"]);
    assert_eq!(expression(&g, "shortestPath", pat), ["s2>t2>2"]);
    assert_eq!(expression(&g, "allShortestPaths", pat), ["s2>t2>2"]);
    // Candidate order does not matter here.
    let g = graph(["s2", "s1"]);
    assert_eq!(expression(&g, "shortestPath", pat), ["s2>t2>2"]);
}

#[test]
fn min_hops_zero_matching_start_and_end_is_the_zero_length_path() {
    let b = Builder::new();
    let (s1, s2) = (b.node("s1", &["S"]), b.node("s2", &["S"]));
    let m = b.node("m", &[]);
    b.chain(&[s1, m, s2]);
    let g = b.done();
    let pat = "(:S)-[*0..]->(:S)";
    assert_eq!(
        clause(&g, "shortestPath", pat),
        ["s1>s1>0", "s1>s2>2", "s2>s2>0"]
    );
    // shortestPath: the first candidate's zero-length path. allShortestPaths:
    // every start matching the end adds its own zero-length path (the
    // min_hops 0 arm takes no length cap), so both, not only the first.
    assert_eq!(expression(&g, "shortestPath", pat), ["s1>s1>0"]);
    assert_eq!(
        expression(&g, "allShortestPaths", pat),
        ["s1>s1>0", "s2>s2>0"]
    );
}

#[test]
fn closed_trails_with_a_shared_start_and_end_variable() {
    // Two starts on separate triangles, of 3 and 2 hops.
    let b = Builder::new();
    let (s1, s2) = (b.node("s1", &["S"]), b.node("s2", &["S"]));
    let [x, y, z] = ["x", "y", "z"].map(|id| b.node(id, &[]));
    b.chain(&[s1, x, y, s1]);
    b.chain(&[s2, z, s2]);
    let g = b.done();
    // Named end bound to the start is a closed pattern per start; the
    // expression form binds s per input row, so run it per row too.
    let rows = exec_cypher(
        &g,
        "MATCH (s:S) RETURN s.id, length(shortestPath((s)-[*1..]->(s)))",
    )
    .rows;
    let mut got: Vec<String> = rows
        .iter()
        .map(|r| r.iter().map(text).collect::<Vec<_>>().join(">"))
        .collect();
    got.sort();
    assert_eq!(got, ["s1>3", "s2>2"]);
    let clause_rows = exec_cypher(
        &g,
        "MATCH (s:S) MATCH p = shortestPath((s)-[*1..]->(s)) RETURN s.id, length(p)",
    )
    .rows;
    let mut clause_rows: Vec<String> = clause_rows
        .iter()
        .map(|r| r.iter().map(text).collect::<Vec<_>>().join(">"))
        .collect();
    clause_rows.sort();
    assert_eq!(clause_rows, got);
}

#[test]
fn input_row_bound_start_agrees_row_by_row() {
    // When every start is bound by a prior clause, the expression form is
    // evaluated once per start, so it agrees with the clause: the
    // divergence needs more than one start candidate inside one pattern.
    let g = graph(["s1", "s2"]);
    let via_clause = exec_cypher(
        &g,
        "MATCH (s:S) MATCH p = shortestPath((s)-[*]->(:T)) \
         RETURN s.id, length(p)",
    )
    .rows;
    let via_expr = exec_cypher(
        &g,
        "MATCH (s:S) RETURN s.id, length(shortestPath((s)-[*]->(:T)))",
    )
    .rows;
    let norm = |rows: &[Vec<Value>]| {
        let mut v: Vec<String> = rows
            .iter()
            .map(|r| r.iter().map(text).collect::<Vec<_>>().join(">"))
            .collect();
        v.sort();
        v
    };
    // The clause with an unbound end returns one row per (s, end) pair.
    assert_eq!(norm(&via_clause), ["s1>3", "s2>1", "s2>2"]);
    // The expression returns one value per s: the shallowest end.
    assert_eq!(norm(&via_expr), ["s1>3", "s2>1"]);
}
