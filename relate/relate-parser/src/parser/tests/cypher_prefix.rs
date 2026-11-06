use super::*;

// ---- CIP-150: CYPHER version prefix ----

#[test]
fn test_cypher_25_prefix_is_transparent() {
    let stmt = parse("CYPHER 25 RETURN 1 AS n").unwrap();
    assert_eq!(stmt.clauses.len(), 1);
    assert!(matches!(stmt.clauses[0], Clause::Return(_)));
    assert_eq!(stmt.cypher_version, Some(25));
}

#[test]
fn test_cypher_5_prefix_is_accepted_and_recorded() {
    // CIP-150: `CYPHER 5` is real, per-statement version selection --
    // parsed and recorded on the statement, not a hard error. Relate
    // still implements a single grammar/engine; version-differentiated
    // behavior (where it exists) is gated at validation/execution time,
    // not by rejecting the preamble itself.
    let stmt = parse("CYPHER 5 RETURN 1 AS n").unwrap();
    assert_eq!(stmt.clauses.len(), 1);
    assert_eq!(stmt.cypher_version, Some(5));
}

#[test]
fn test_cypher_no_prefix_leaves_version_unset() {
    let stmt = parse("RETURN 1 AS n").unwrap();
    assert_eq!(stmt.cypher_version, None);
}

#[test]
fn test_cypher_unknown_version_number_is_rejected() {
    assert!(parse("CYPHER 99 RETURN 1 AS n").is_err());
}

#[test]
fn test_cypher_decimal_version_is_rejected() {
    assert!(parse("CYPHER 5.0 RETURN 1 AS n").is_err());
}

#[test]
fn test_cypher_variable_named_cypher_is_unaffected() {
    // The CYPHER-prefix check only fires once, at the very start of
    // the whole query -- `cypher` used as an ordinary variable name
    // anywhere else must keep working.
    let stmt = parse("WITH 1 AS cypher RETURN cypher").unwrap();
    assert_eq!(stmt.clauses.len(), 2);
}

#[test]
fn test_cypher_same_version_repeated_is_accepted() {
    let stmt = parse("CYPHER 5 CYPHER 5 RETURN 1 AS n").unwrap();
    assert_eq!(stmt.cypher_version, Some(5));
}

#[test]
fn test_cypher_conflicting_versions_are_rejected() {
    assert!(parse("CYPHER 5 CYPHER 25 RETURN 1 AS n").is_err());
}

#[test]
fn test_cypher_versionless_options_clause_is_accepted() {
    let stmt = parse("CYPHER planner=cost RETURN 1 AS n").unwrap();
    assert_eq!(stmt.cypher_version, None);
}

#[test]
fn test_cypher_unrecognized_option_key_is_rejected() {
    assert!(parse("CYPHER 5 foo=bar RETURN 1 AS n").is_err());
}

#[test]
fn test_cypher_conflicting_option_values_are_rejected() {
    assert!(parse("CYPHER 5 runtime=slotted runtime=pipelined RETURN 1 AS n").is_err());
}

#[test]
fn test_cypher_explain_before_version_is_accepted() {
    let stmt = parse("EXPLAIN CYPHER 5 runtime=slotted RETURN 1 AS n").unwrap();
    assert!(stmt.explain);
    assert_eq!(stmt.cypher_version, Some(5));
}

#[test]
fn test_cypher_version_before_explain_is_accepted() {
    let stmt = parse("CYPHER 5 runtime=slotted EXPLAIN RETURN 1 AS n").unwrap();
    assert!(stmt.explain);
    assert_eq!(stmt.cypher_version, Some(5));
}

#[test]
fn test_cypher_explain_inside_options_clause_is_rejected() {
    // Doc example E1: once EXPLAIN has ended a CYPHER options clause,
    // a bare `key=value` with no fresh CYPHER keyword falls through to
    // ordinary statement parsing and is a generic syntax error there.
    assert!(parse("CYPHER 5 EXPLAIN runtime=slotted RETURN 1 AS n").is_err());
}

// ---- Cypher 25 header + runtime option value validation (regression pins) ----

#[test]
fn test_cypher_25_runtime_parallel_is_accepted_and_ignored() {
    // The documented parallel-runtime header: accepted, recorded as
    // version 25, and ignored (Relate has a single engine).
    let stmt = parse("CYPHER 25 runtime=parallel RETURN 1 AS n").unwrap();
    assert_eq!(stmt.cypher_version, Some(25));
    assert!(matches!(stmt.clauses[0], Clause::Return(_)));
}

#[test]
fn test_cypher_25_runtime_unknown_value_is_rejected_naming_valid_values() {
    // The value-side twin of the 22N07 key check: an unknown runtime
    // value fails with an authored 22N10 error naming the valid values.
    let err = parse("CYPHER 25 runtime=bogus RETURN 1 AS n").expect_err("expected a value error");
    assert!(
        err.message
            .starts_with("SyntaxError: 22N10 invalid pre-parser option value"),
        "wrong error: {}",
        err.message
    );
    assert!(
        err.message.contains("parallel"),
        "error must name the valid values: {}",
        err.message
    );
}

#[test]
fn test_cypher_runtime_option_accepts_the_runtime_vocabulary() {
    // The full runtime vocabulary of Neo4j's current line (its manual's
    // three runtimes); all accepted-and-ignored, under either version.
    for q in [
        "CYPHER 25 runtime=slotted RETURN 1 AS n",
        "CYPHER 25 runtime=pipelined RETURN 1 AS n",
        "CYPHER 5 runtime=parallel RETURN 1 AS n",
        "CYPHER 5 runtime=slotted RETURN 1 AS n",
        "CYPHER 5 runtime=pipelined RETURN 1 AS n",
        "CYPHER runtime=parallel RETURN 1 AS n",
    ] {
        let stmt = parse(q).unwrap_or_else(|e| panic!("{q} must parse: {e}"));
        assert!(matches!(stmt.clauses[0], Clause::Return(_)), "{q}");
    }
}

#[test]
fn test_cypher_25_unrecognized_option_key_is_rejected() {
    // The 22N07 key check under the Cypher 25 preamble too.
    let err = parse("CYPHER 25 foo=bar RETURN 1 AS n").expect_err("expected a key error");
    assert!(
        err.message
            .starts_with("SyntaxError: 22N07 invalid pre-parser option key"),
        "wrong error: {}",
        err.message
    );
}
