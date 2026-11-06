use super::*;

// -- CIP-135: WHEN/THEN/ELSE conditional clause --

#[test]
fn test_when_then_else_parses_as_sole_conditional_clause() {
    let stmt = parse(
        "WHEN true THEN { RETURN 1 AS n } WHEN false THEN { RETURN 2 AS n } ELSE { RETURN 3 AS n }",
    )
    .unwrap();
    assert_eq!(stmt.clauses.len(), 1);
    match &stmt.clauses[0] {
        Clause::Conditional(cc) => {
            assert_eq!(cc.branches.len(), 2);
            assert!(cc.else_body.is_some());
        }
        other => panic!("expected Conditional clause, got {other:?}"),
    }
}

#[test]
fn test_when_without_else_is_ok() {
    let stmt = parse("WHEN true THEN { RETURN 1 AS n }").unwrap();
    match &stmt.clauses[0] {
        Clause::Conditional(cc) => {
            assert_eq!(cc.branches.len(), 1);
            assert!(cc.else_body.is_none());
        }
        other => panic!("expected Conditional clause, got {other:?}"),
    }
}

#[test]
fn test_bare_when_mid_statement_is_a_syntax_error() {
    // A `WHEN` at clause-start is only a `<when query>` at the very
    // start of a `<regular query>` -- after MATCH it must fall through
    // to `parse_clause_inner`'s catch-all.
    assert!(parse("MATCH (n) WHEN true THEN { RETURN 1 AS s } RETURN n").is_err());
}

#[test]
fn test_failing_when_branch_body_restores_in_when_branch_body() {
    for q in [
        "WHEN true THEN MATCH (n) RETURN",
        "WHEN true THEN RETURN 1 AS n ELSE MATCH (n) RETURN",
    ] {
        let mut parser =
            Parser::new(q, "query text", crate::lexer::ParseOptions::default()).unwrap();
        assert!(parser.parse_when_query().is_err(), "{q:?}");
        assert_eq!(parser.in_when_branch_body, 0, "{q:?}");
    }
}

#[test]
fn test_lone_else_with_no_when_is_a_syntax_error() {
    assert!(parse("ELSE { RETURN 1 AS n }").is_err());
}

#[test]
fn test_when_after_else_in_same_chain_is_a_syntax_error() {
    assert!(parse(
        "WHEN true THEN { RETURN 1 AS n } ELSE { RETURN 2 AS n } WHEN true THEN { RETURN 3 AS n }"
    )
    .is_err());
}

#[test]
fn test_bare_when_chain_cannot_be_unioned() {
    assert!(
        parse("WHEN true THEN { RETURN 1 AS n } UNION WHEN true THEN { RETURN 2 AS n }").is_err()
    );
}

#[test]
fn test_braced_when_chain_can_be_unioned() {
    let stmt =
        parse("{ WHEN true THEN { RETURN 1 AS n } } UNION { WHEN false THEN { RETURN 2 AS n } }")
            .unwrap();
    assert!(matches!(stmt.clauses[0], Clause::Conditional(_)));
    assert!(stmt.union.is_some());
}

#[test]
fn test_two_adjacent_braced_when_blocks_with_no_connector_is_an_error() {
    assert!(parse(
        "{ WHEN true THEN { RETURN 1 AS n } } { WHEN true THEN { RETURN 2 AS n } } RETURN n"
    )
    .is_err());
}

#[test]
fn test_when_inside_call_subquery_parses() {
    let stmt =
        parse("CALL () { WHEN true THEN { RETURN 1 AS s } ELSE { RETURN 2 AS s } } RETURN s")
            .unwrap();
    match &stmt.clauses[0] {
        Clause::CallSubquery(csq) => {
            assert!(matches!(csq.statement.clauses[0], Clause::Conditional(_)));
        }
        other => panic!("expected CallSubquery clause, got {other:?}"),
    }
}

// A `{ WHEN ... THEN RETURN ... ELSE RETURN ... }` UNION arm projects the
// columns its branch bodies project; the EXISTS/COUNT/COLLECT union-shape
// screen must classify it by those columns, not as a branch that omits
// RETURN (which reported the mixed-shape error for a valid query).
#[test]
fn test_scoped_when_union_arm_counts_as_returning_in_exists_body() {
    for q in [
        "RETURN *, EXISTS { RETURN 1 AS x UNION { WHEN true THEN RETURN 2 AS x ELSE RETURN 3 AS x } } AS result",
        "RETURN *, COUNT { RETURN 1 AS x UNION { WHEN true THEN RETURN 2 AS x ELSE RETURN 3 AS x } } AS result",
    ] {
        parse(q).unwrap_or_else(|e| panic!("{q}: {e:?}"));
    }
}

/// LABS-712: an error raised inside a recognised `CASE` must propagate as
/// itself. The old backtracking swallowed it, re-read `CASE` as a variable
/// and reported `unexpected token "WHEN"` instead of the real error.
#[test]
fn test_case_propagates_errors_from_inside_a_recognised_case() {
    let err = parse("RETURN CASE WHEN false THEN 9223372036854775808 ELSE 1 END AS v")
        .expect_err("the overflowing literal is a compile-time error");
    assert_eq!(err.detail, Some("IntegerOverflow"), "got: {err:?}");
    assert!(
        err.message.starts_with("integer overflow"),
        "message drifted: {}",
        err.message
    );
}

/// The fallback to the identifier reading still applies when the CASE
/// grammar was not entered, and real CASE expressions are unaffected.
#[test]
fn test_case_identifier_and_case_expression_both_still_parse() {
    for q in [
        "RETURN CASE WHEN true THEN 1 ELSE 2 END AS v",
        "RETURN CASE 1 WHEN 1 THEN 'one' END AS v",
        "WITH 1 AS case RETURN case AS v",
        "WITH {name: 'x'} AS case RETURN case {name: case.name} AS v",
    ] {
        parse(q).unwrap_or_else(|e| panic!("{q}: {e:?}"));
    }
}

/// LABS-712: the CASE-vs-variable decision must not misfire on any valid
/// simple-CASE operand or on any operator/postfix continuation of a
/// variable named `case`.
#[test]
fn test_case_grammar_lookahead_keeps_valid_operands_and_case_variables() {
    for q in [
        // Simple-form test operands that are not plain identifiers.
        "RETURN CASE [1, 2] WHEN [1, 2] THEN 'x' ELSE 'y' END AS v",
        "RETURN CASE -10 WHEN -10 THEN 'minus ten' ELSE 'else' END AS v",
        "RETURN CASE 'show' WHEN 'show' THEN 1 ELSE 0 END AS v",
        "LET show = 1 RETURN CASE show WHEN 1 THEN 'one' ELSE 'no' END AS v",
        // `case` as an ordinary variable, continued by operators and
        // postfix forms.
        "WITH 7 AS case RETURN case - 1 AS v",
        "WITH 7 AS case RETURN case + 1 AS v",
        "WITH 7 AS case RETURN case IN [7] AS v",
        "WITH 7 AS case RETURN case IS NULL AS v",
        "WITH 7 AS case RETURN case AND true AS v",
        "WITH [1, 2] AS case RETURN case[0] AS v",
        "WITH {name: 'x'} AS case RETURN case {name: case.name} AS v",
    ] {
        assert!(parse(q).is_ok(), "{q} must parse");
    }
}
