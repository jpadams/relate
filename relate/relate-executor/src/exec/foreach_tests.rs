//! The FOREACH side-effect boundary (dispatch): the body runs for its
//! writes only, and the admitted clause set (the updating clauses) is the
//! front's `validate_foreach_body` rule, applied before lowering — the one
//! home of the rule (`exec_foreach` keeps no second copy, which would have
//! to re-spell the classification over the IR clause vocabulary).
//! Everything the body produces — a void CALL's rows, a conditional's
//! merge — is consumed by the boundary by design.

use relate_core::Value;
use relate_front::parse_and_lower;

use super::test_support::{exec_cypher_on, write_scratch};
use crate::graph::Graph;
use crate::overlay::ExecGraph;

/// Neo4j iterates a non-list expression once (as a one-element list) and a
/// null expression zero times — the harness pins both spellings
/// (`FOREACH(v IN 1 | …)` and `FOREACH(v IN <boolean> | …)`).
#[test]
fn a_non_list_expression_iterates_once_and_null_never() {
    let g = Graph::default();
    let scratch = write_scratch(&g);
    let view = ExecGraph::write(&g, &scratch);
    exec_cypher_on(&view, "FOREACH (v IN 1 | CREATE ({property: v}))");
    exec_cypher_on(
        &view,
        "FOREACH (v IN true and false xor true or false = false | CREATE ({property: v}))",
    );
    exec_cypher_on(&view, "FOREACH (v IN null | CREATE ())");
    assert_eq!(
        exec_cypher_on(&view, "MATCH (n) RETURN count(n) AS c").rows,
        vec![vec![Value::Int(2)]]
    );
}

/// The composition is rejected by the front's validate battery before the
/// planner or executor ever see it (`exec_foreach` applies no second copy:
/// the rule lives once, over the parser AST it needs).
#[test]
fn a_return_inside_a_foreach_body_fails_closed_at_the_front() {
    let err = parse_and_lower("FOREACH (i IN [1] | RETURN 1)")
        .expect_err("the RETURN guard rejects the body before execution");
    assert_eq!(
        err.to_string(),
        "syntax error: InvalidClauseComposition: invalid FOREACH: invalid use of RETURN inside 'FOREACH'"
    );
}
