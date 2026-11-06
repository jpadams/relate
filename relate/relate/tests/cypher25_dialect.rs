//! Cypher 25 dialect regression pins (LABS-642): every Cypher-25 form the
//! 2026-10-31 dialect spike verified as parsing *and executing* on the
//! real engine, pinned end to end so none can silently regress -- the
//! spike found all of them working but none of them tested.
//!
//! Oracle values come from the Neo4j-verified CIP-51/CIP-60 cip-eval
//! corpus — the spec harness checkout's
//! `features/cip-eval/CIP-51-gpm-path-modes/` (ADR-0520): the background
//! graph and each path mode's expected `nodes(p)` lists below are that
//! corpus's single-path-pattern scenarios, re-stated as Rust pins.
//!
//! Pinned here (execution level; the parser-side shape pins live in
//! `relate-parser`'s `patterns`/`qpp`/`expressions`/`cypher_prefix`
//! test modules): the WALK/TRAIL/SIMPLE/ACYCLIC path-mode family, the
//! `p = REPEATABLE ELEMENTS (...)` spelling, the allReduce three-argument
//! expression form, the `CYPHER 25` header (with `runtime=` value
//! validation), NEXT chains carrying rows between stages, zero-lower-bound
//! quantified path patterns, and chained comparisons.

mod common;

use common::{connection, err_text, row, rows, scalar_one};
use relate::Value;

/// The CIP-51 corpus's background graph (its single-path-pattern
/// scenarios' node/relationship `i` values are the oracles below).
fn doc_graph() -> relate::Connection {
    let mut c = connection();
    common::run(
        &mut c,
        "CREATE (N3:A {i: 1, p: 2})-[:R3 {i: 1}]->(N1:A:B {i: 2, p: 2})-[:R1 {i: 2}]->(:A {i: 3, p: 2})-[:R2 {i: 3}]->(N3),
                (N1)-[:R4 {i: 4}]->(N4:A {i: 4, p: 2})-[:R5 {i: 5}]->(N1),
                (:A {i: 5, p: 3})<-[:R999 {i: 6}]-(N4)-[:R99 {i: 7}]->(:B {i: 6, p: 4})",
    );
    c
}

/// The sorted `nodes(p)` `i`-lists of a path query -- the corpus's row
/// oracle shape, order-insensitive.
fn node_lists(c: &mut relate::Connection, query: &str) -> Vec<Vec<i64>> {
    let mut lists: Vec<Vec<i64>> = rows(c, query)
        .into_iter()
        .map(|r| match &r[0] {
            Value::List(items) => items
                .iter()
                .map(|v| match v {
                    Value::Int(i) => *i,
                    other => panic!("{query}: non-integer node value {other:?}"),
                })
                .collect(),
            other => panic!("{query}: expected a node list, got {other:?}"),
        })
        .collect();
    lists.sort();
    lists
}

/// The corpus's verified `nodes(p)` lists for
/// `MATCH [mode prefix] (:B)((a)-->(b:A WHERE b.p = a.p)){1,4}` on the
/// background graph, per path mode.
fn expected_node_lists(mode: &str) -> Vec<Vec<i64>> {
    let mut lists: Vec<Vec<i64>> = match mode {
        // CIP-51 single-path-pattern [2] (explicit WALK under REPEATABLE
        // ELEMENTS): repeated nodes and relationships permitted.
        "walk" => vec![
            vec![2, 3],
            vec![2, 4],
            vec![2, 3, 1],
            vec![2, 4, 2],
            vec![2, 3, 1, 2],
            vec![2, 4, 2, 3],
            vec![2, 4, 2, 4],
            vec![2, 4, 2, 3, 1],
            vec![2, 4, 2, 4, 2],
            vec![2, 3, 1, 2, 3],
            vec![2, 3, 1, 2, 4],
        ],
        // CIP-51 single-path-pattern [4]: no repeated relationships.
        "trail" => vec![
            vec![2, 3],
            vec![2, 4],
            vec![2, 3, 1],
            vec![2, 4, 2],
            vec![2, 3, 1, 2],
            vec![2, 4, 2, 3],
            vec![2, 4, 2, 3, 1],
            vec![2, 3, 1, 2, 4],
        ],
        // CIP-51 single-path-pattern [9] (@doc-derived): only the first
        // and last node of a path may repeat.
        "simple" => vec![
            vec![2, 3],
            vec![2, 4],
            vec![2, 3, 1],
            vec![2, 4, 2],
            vec![2, 3, 1, 2],
        ],
        // CIP-51 single-path-pattern [8]: no repeated nodes at all.
        "acyclic" => vec![vec![2, 3], vec![2, 4], vec![2, 3, 1]],
        other => panic!("unknown path mode {other:?}"),
    };
    lists.sort();
    lists
}

#[test]
fn walk_trail_simple_acyclic_path_modes_execute_the_corpus_rows() {
    let mut c = doc_graph();
    for (mode, prefix) in [
        // WALK is only distinct under REPEATABLE ELEMENTS; under the
        // default match mode DIFFERENT RELATIONSHIPS dominates it
        // (CIP-51 single-path-pattern [7]).
        ("walk", "MATCH REPEATABLE ELEMENTS p = WALK"),
        ("trail", "MATCH p = TRAIL"),
        ("simple", "MATCH p = SIMPLE"),
        ("acyclic", "MATCH p = ACYCLIC"),
    ] {
        let q = format!(
            "{prefix} (:B)((a)-->(b:A WHERE b.p = a.p)){{1,4}} \
             RETURN [n IN nodes(p) | n.i] AS ns"
        );
        assert_eq!(
            node_lists(&mut c, &q),
            expected_node_lists(mode),
            "{mode}: path mode rows"
        );
    }
}

#[test]
fn repeatable_elements_path_variable_first_spelling_executes() {
    // `MATCH p = REPEATABLE ELEMENTS (pattern)`: the path variable binds
    // and the clause executes with relaxed relationship uniqueness --
    // the same rows the corpus pins for the match-mode-first spelling.
    let mut c = doc_graph();
    let var_first = node_lists(
        &mut c,
        "MATCH p = REPEATABLE ELEMENTS (:B)((a)-->(b:A WHERE b.p = a.p)){1,4} \
         RETURN [n IN nodes(p) | n.i] AS ns",
    );
    assert_eq!(var_first, expected_node_lists("walk"));
    // ...and identical rows to the clause-initial spelling of the same
    // clause (the corpus's own order).
    let mode_first = node_lists(
        &mut c,
        "MATCH REPEATABLE ELEMENTS p = (:B)((a)-->(b:A WHERE b.p = a.p)){1,4} \
         RETURN [n IN nodes(p) | n.i] AS ns",
    );
    assert_eq!(var_first, mode_first);
}

#[test]
fn repeatable_elements_permits_relationship_equijoins() {
    // The Neo4j manual's own oracle: a repeated relationship variable can
    // only produce paths under REPEATABLE ELEMENTS (four traversals of
    // the two-relationship chain), never under the default match mode.
    let mut c = connection();
    common::run(
        &mut c,
        "CREATE (a:A), (b:B), (c:C), (a)-[:T]->(b), (b)-[:T]->(c)",
    );
    for (q, want) in [
        (
            "MATCH p = REPEATABLE ELEMENTS ()-[r]-()-[r]-() RETURN count(*) AS n",
            4,
        ),
        (
            "MATCH REPEATABLE ELEMENTS p = ()-[r]-()-[r]-() RETURN count(*) AS n",
            4,
        ),
        ("MATCH p = ()-[r]-()-[r]-() RETURN count(*) AS n", 0),
    ] {
        assert_eq!(
            scalar_one(&mut c, q),
            Value::Int(want),
            "{q}: relationship equijoin"
        );
    }
}

#[test]
fn allreduce_three_argument_form_gates_on_its_predicate() {
    // The Cypher 25 allReduce expression form: accumulator, update, and
    // keep-going predicate. The predicate decides whether the reduction
    // completes over the whole list.
    let mut c = connection();
    assert_eq!(
        scalar_one(
            &mut c,
            "RETURN allReduce(s = 0, x IN [1,2,3] | s + x, s < 7) AS r"
        ),
        Value::Bool(true)
    );
    assert_eq!(
        scalar_one(
            &mut c,
            "RETURN allReduce(s = 0, x IN [1,2,3] | s + x, s < 3) AS r"
        ),
        Value::Bool(false)
    );
}

#[test]
fn cypher_25_header_is_accepted_and_runtime_option_values_are_validated() {
    let mut c = connection();
    // The header itself is transparent; `runtime=parallel` is
    // accepted-and-ignored (Relate has a single engine).
    for q in [
        "CYPHER 25 RETURN 1 AS n",
        "CYPHER 25 runtime=parallel RETURN 1 AS n",
    ] {
        assert_eq!(row(&mut c, q), vec![Value::Int(1)], "{q}");
    }
    // An unknown runtime value fails with the authored 22N10 error naming
    // the valid values.
    let err = err_text(c.query("CYPHER 25 runtime=bogus RETURN 1 AS n"));
    assert!(
        err.contains("22N10 invalid pre-parser option value") && err.contains("parallel"),
        "wrong error text: {err}"
    );
    // And an unknown option key keeps its authored 22N07 error.
    let err = err_text(c.query("CYPHER 25 foo=bar RETURN 1 AS n"));
    assert!(
        err.contains("22N07 invalid pre-parser option key"),
        "wrong error text: {err}"
    );
}

#[test]
fn next_chains_carry_rows_between_stages() {
    let mut c = connection();
    assert_eq!(
        row(&mut c, "RETURN 1 AS a NEXT WITH a AS b RETURN b + 1 AS c"),
        vec![Value::Int(2)],
        "WITH-stage NEXT chain"
    );
    assert_eq!(
        row(
            &mut c,
            "RETURN 1 AS a NEXT RETURN a + 1 AS b NEXT RETURN b * 2 AS c"
        ),
        vec![Value::Int(4)],
        "three-stage NEXT chain"
    );
}

#[test]
fn zero_lower_bound_quantified_path_patterns_match_classic_var_length() {
    // `{0,n}` and `{0,}` include the zero-iteration path (the anchor
    // binds the trailing node too) and agree with the classic
    // `*0..n`/`*0..` var-length spelling on the same query.
    let mut c = connection();
    common::run(
        &mut c,
        "CREATE (n1 {id: 1})-[:R]->(n2 {id: 2})-[:R]->(n3 {id: 3})",
    );
    let classic = rows(
        &mut c,
        "MATCH (a {id: 1})-[:R*0..2]->(b) RETURN b.id AS id ORDER BY id",
    );
    for q in [
        "MATCH (a {id: 1})((x)-[:R]->(y)){0,2}(b) RETURN b.id AS id ORDER BY id",
        "MATCH (a {id: 1})((x)-[:R]->(y)){0,}(b) RETURN b.id AS id ORDER BY id",
    ] {
        assert_eq!(rows(&mut c, q), classic, "{q}: zero-lower-bound QPP");
    }
    // The zero-iteration row is really there: the anchor reaches itself.
    assert_eq!(
        classic,
        vec![
            vec![Value::Int(1)],
            vec![Value::Int(2)],
            vec![Value::Int(3)]
        ]
    );
}

#[test]
fn chained_comparisons_evaluate_as_a_conjunction() {
    // `a <= b <= c` is `(a <= b) AND (b <= c)`, in either direction and
    // with any false link breaking the chain.
    let mut c = connection();
    assert_eq!(
        row(
            &mut c,
            "RETURN 1 <= 2 <= 3 AS asc, 3 >= 2 >= 1 AS desc, \
             2 >= 3 >= 1 AS broken_mid, 1 <= 0 <= 1 AS broken_left"
        ),
        vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false)
        ]
    );
}
