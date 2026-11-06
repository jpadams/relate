//! Pattern expressions in boolean positions: the planner's predicate-shape
//! rule allows a pattern where a predicate is read (`NOT`, `AND`/`OR`/`XOR`,
//! a `CASE WHEN` condition) even in a projection, and a bare pattern value
//! stays a front-end rejection.

fn planned(src: &str) -> Result<relate_planner::LogicalPlan, relate_core::Error> {
    let query = relate_front::parse_and_lower(src)?;
    relate_planner::plan(query, &mut |_| false)
}

#[test]
fn pattern_expressions_are_legal_in_boolean_positions() {
    planned("MATCH (n) RETURN NOT (n)--() AS x").unwrap();
    planned("MATCH (n) RETURN (n)--() OR (n)--()--() AS x").unwrap();
    planned("MATCH (n) RETURN CASE WHEN (n)--() THEN 1 ELSE 0 END AS x").unwrap();
}

#[test]
fn a_bare_pattern_projection_is_rejected() {
    assert!(planned("MATCH (n) RETURN (n)--() AS x").is_err());
}
