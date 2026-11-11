use super::*;

// ---- ADR-0576 Decision 2: the CYPHER preamble clause is removed ----

/// A statement-opening `CYPHER` clause fails with the one authored typed
/// error, whatever follows the keyword.
fn assert_prefix_rejected(q: &str) {
    let err = parse(q).unwrap_err();
    assert_eq!(
        err.detail,
        Some(UNSUPPORTED_CYPHER_PREFIX_DETAIL),
        "{q:?}: wrong typed error: {}",
        err.message
    );
}

#[test]
fn test_cypher_prefix_with_a_version_is_rejected() {
    // Both spellings a reference server ships, and any other integer —
    // the rejection is uniform, not the old 22N10 value check.
    for q in [
        "CYPHER 5 RETURN 1 AS n",
        "CYPHER 25 RETURN 1 AS n",
        "CYPHER 4 RETURN 1 AS n",
        // A decimal version and a repeated clause fail the same way.
        "CYPHER 5.0 RETURN 1 AS n",
        "CYPHER 5 CYPHER 25 RETURN 1 AS n",
    ] {
        assert_prefix_rejected(q);
    }
}

#[test]
fn test_cypher_options_clause_without_a_version_is_rejected() {
    // The whole preamble clause is gone, not just the version arm: a
    // version-less options clause (or a bare keyword) fails with the same
    // typed error, never the old 22N07/22N10 key/value errors.
    for q in [
        "CYPHER runtime=slotted RETURN 1 AS n",
        "CYPHER planner=cost RETURN 1 AS n",
        "CYPHER 5 foo=bar RETURN 1 AS n",
        "CYPHER 25 runtime=bogus RETURN 1 AS n",
        "CYPHER",
    ] {
        assert_prefix_rejected(q);
    }
}

#[test]
fn test_cypher_prefix_after_explain_or_profile_is_rejected() {
    // The preamble position runs after the EXPLAIN/PROFILE flags, so the
    // rejection covers that spelling too — before any other processing.
    assert_prefix_rejected("EXPLAIN CYPHER 5 RETURN 1 AS n");
    assert_prefix_rejected("PROFILE CYPHER 25 runtime=slotted RETURN 1 AS n");
}

#[test]
fn test_cypher_prefix_is_case_insensitive() {
    assert_prefix_rejected("cypher 5 RETURN 1 AS n");
}

#[test]
fn test_explain_and_profile_still_parse() {
    let stmt = parse("EXPLAIN RETURN 1 AS n").unwrap();
    assert!(stmt.explain);
    assert!(!stmt.profile);
    let stmt = parse("PROFILE RETURN 1 AS n").unwrap();
    assert!(!stmt.explain);
    assert!(stmt.profile);
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
fn test_nested_cypher_is_an_ordinary_syntax_error() {
    // Only the statement preamble position carries the authored
    // rejection; a `CYPHER` clause inside a nested body was never a
    // preamble and keeps the generic unexpected-token error.
    let err = parse("CALL { CYPHER 5 RETURN 1 AS n } RETURN 1 AS x").unwrap_err();
    assert_ne!(
        err.detail,
        Some(UNSUPPORTED_CYPHER_PREFIX_DETAIL),
        "a nested CYPHER is not a statement preamble: {}",
        err.message
    );
}
