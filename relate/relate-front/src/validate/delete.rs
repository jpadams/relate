//! DELETE-target admission policy: which expression forms may stand as a
//! DELETE target (the `check_delete_expr` walk), extracted from
//! `expressions.rs` as its own concern module — the `match_shape.rs`
//! precedent. The scope-coupled clause dispatch (`validate_delete_clause`,
//! undefined-variable checks included) stays in `expressions.rs`; only the
//! per-expression policy and its test battery live here.

use relate_error::{EngineError, ErrorKind};
use relate_parser::{Expr, LiteralValue};

/// Base sentence of every DELETE-target diagnostic. The HasLabel and
/// generic arms use it verbatim; the CIP-55 keyword-literal arm appends
/// the `inf`/`infinity`/`nan` explanation to it.
const DELETE_TARGET_MSG: &str = "DELETE requires a node or relationship, not an expression";

/// Check that a DELETE expression is a valid deletable target.
///
/// The one stated rule, shared with the executor's runtime half
/// (`relate-executor`'s `exec::write::collect_delete`, whose doc cites
/// this one): static validation walks only the expression forms that
/// statically carry deletable *slots* — variables, property/subscript
/// access, parentheses, `CASE` result branches (CIP-118 requires DELETE to
/// accept "any expression, including subquery expressions", e.g. `DELETE
/// (CASE WHEN EXISTS {...} THEN q ELSE p END)`; the subquery driving the
/// condition is nested inside a `WHEN`, not itself a target) and list
/// *elements* — and it is permissive about every form whose value is
/// computed at runtime: function calls and the subquery/comprehension
/// forms. The runtime fold is the single authority on what a *resolved*
/// DELETE value is — it raises the clause's type error for any value that
/// is not a node, relationship, path, list thereof, or null. Only
/// expression forms *statically certain* to never produce a deletable
/// value (arithmetic, boolean/comparison operators, non-null literals,
/// label predicates, map literals/projections, pattern predicates) are
/// rejected here at compile time, matching TCK
/// `clauses/delete/Delete5.feature` scenario 9 (`DELETE 1 + 1` must be a
/// compile-time `InvalidArgumentType` SyntaxError, even against an empty
/// graph where DELETE never actually runs).
pub(crate) fn check_delete_expr(expr: &Expr) -> Result<(), EngineError> {
    match expr {
        Expr::Variable(_) => Ok(()),
        Expr::Property(_) => Ok(()),
        Expr::Subscript(_) => Ok(()),
        Expr::Paren(inner) => check_delete_expr(inner),
        // A list target deletes its elements (Delete5 scenarios 1/2/7
        // delete through lists of nodes, relationships, and paths), so each
        // element is a delete target in its own right — the same
        // per-element walk the `CASE` arm runs over its result branches.
        Expr::List(items) => {
            for item in items {
                check_delete_expr(item)?;
            }
            Ok(())
        }
        // `DELETE null` is a legal no-op: null is the one literal whose
        // runtime value can never be a non-deletable entity, and the
        // executor's `delete_values` fold already ignores `Value::Null`. The
        // Neo4j acceptance suite (`Finish.feature`) pairs `DELETE null`
        // with `FINISH`; the TCK's `DELETE 1 + 1` rejection
        // (Delete5 scenario 9) is unaffected because that is a *numeric*
        // literal, still caught by the catch-all arm below.
        Expr::Literal(LiteralValue::Null) => Ok(()),
        Expr::Case(c) => {
            for w in &c.whens {
                check_delete_expr(&w.result)?;
            }
            if let Some(e) = &c.else_expr {
                check_delete_expr(e)?;
            }
            Ok(())
        }
        // Computed forms, permissive per the rule: the resolved value — a
        // node from `coalesce(a, b)`, a path from `shortestPath(...)`, a
        // list of paths from a pattern comprehension, whatever a subquery
        // expression returns — is judged by the runtime fold, the single
        // rejection authority.
        Expr::FunctionCall(_)
        | Expr::ExistsSubquery(_)
        | Expr::CountSubquery(_)
        | Expr::CollectSubquery(_)
        | Expr::ListComprehension(_)
        | Expr::MapComprehension(_)
        | Expr::PatternComprehension(_)
        | Expr::Reduce(_)
        | Expr::AllReduce(_)
        | Expr::ShortestPath(_) => Ok(()),
        // `DELETE n:Person` / `DELETE r:T` parse as a label-predicate boolean
        // expression; Neo4j rejects these as a compile-time `SyntaxError:
        // InvalidDelete` (TCK clauses/delete Delete1 scenario 8, Delete2
        // scenario 5) — a distinct detail from the generic non-entity case.
        Expr::HasLabel(_) => Err(EngineError::semantic(
            ErrorKind::InvalidDelete,
            DELETE_TARGET_MSG.to_string(),
        )),
        // A bare `inf`/`infinity`/`nan` DELETE target parses as the CIP-55
        // float literal (parse_primary lowers the INFINITY/NAN keyword tokens
        // unconditionally). The parser rejects overflowing numeric literals
        // like `1e400`, so a *non-finite* Float literal in the AST can only
        // have come from one of those keyword tokens — name the cause and the
        // backtick-escape workaround (ADR-0280). The error stays a
        // compile-time `InvalidArgumentType`, as TCK Delete5 scenario 9 pins.
        Expr::Literal(LiteralValue::Float(f)) if !f.is_finite() => Err(EngineError::semantic(
            ErrorKind::InvalidArgumentType,
            format!(
                "{DELETE_TARGET_MSG}: `inf`, `infinity` and `nan` are CIP-55 float-literal \
                 keywords (Infinity/NaN), not variables; backtick-escape the name (e.g. DELETE \
                 `inf`) to delete a variable of that name"
            ),
        )),
        // Any other non-entity expression (`DELETE 1 + 1`) is a compile-time
        // `SyntaxError: InvalidArgumentType` (TCK Delete5 scenario 9).
        _ => Err(EngineError::semantic(
            ErrorKind::InvalidArgumentType,
            DELETE_TARGET_MSG.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::parse_and_validate_expression_constraints;

    /// Parse `query`, require the static rejection, and pin its error
    /// class — the one spelling of the reject-and-assert-kind block the
    /// `InvalidArgumentType` rejections start from (this module's every
    /// static DELETE rejection carries that class; TCK Delete5 scenario 9
    /// pins it). Each test then asserts only what distinguishes it
    /// (message text, variant, reachability).
    fn rejected_statically(query: &str) -> EngineError {
        let err = parse_and_validate_expression_constraints(query)
            .expect_err(&format!("`{query}` must be rejected statically"));
        assert_eq!(
            err.kind(),
            ErrorKind::InvalidArgumentType,
            "`{query}` keeps the Delete5 scenario 9 error class"
        );
        err
    }

    // ADR-0280 ruling 2: a bare `inf`/`infinity`/`nan` DELETE target is the
    // CIP-55 float literal, not a variable; the static rejection names the
    // keyword and prescribes the backtick escape.
    #[test]
    fn delete_keyword_literal_target_names_backtick_escape() {
        for kw in ["inf", "infinity", "nan", "INF", "Infinity", "NaN"] {
            let err = parse_and_validate_expression_constraints(&format!("MATCH (n) DELETE {kw}"))
                .expect_err(&format!("DELETE {kw} must be rejected"));
            assert!(
                matches!(err, EngineError::Semantic { .. }),
                "DELETE {kw} must fail at compile time (Semantic), got {err:?}"
            );
            assert_eq!(
                err.kind(),
                ErrorKind::InvalidArgumentType,
                "DELETE {kw} keeps the TCK Delete5 scenario 9 error class"
            );
            assert_eq!(
                err.to_string(),
                "InvalidArgumentType: DELETE requires a node or relationship, not an \
                 expression: `inf`, `infinity` and `nan` are CIP-55 float-literal keywords \
                 (Infinity/NaN), not variables; backtick-escape the name (e.g. DELETE `inf`) \
                 to delete a variable of that name",
                "DELETE {kw} message"
            );
        }
    }

    #[test]
    fn delete_keyword_literal_diagnostic_through_parens_and_detach() {
        // The dedicated arm is reached through Paren unwrapping and DETACH.
        for query in [
            "MATCH (n) DELETE (inf)",
            "MATCH (n) DETACH DELETE nan",
            "OPTIONAL MATCH (n)-[:R]->(inf:N) DETACH DELETE n, inf",
        ] {
            let err = rejected_statically(query);
            assert!(
                err.to_string().contains("CIP-55 float-literal keywords"),
                "`{query}` must carry the keyword diagnostic, got {err}"
            );
        }
    }

    #[test]
    fn delete_non_literal_expression_keeps_generic_message() {
        // TCK Delete5 scenario 9 pins `DELETE 1 + 1`: compile-time
        // InvalidArgumentType with the generic message (no keyword note).
        for query in ["MATCH () DELETE 1 + 1", "MATCH (n) DELETE 1.5"] {
            let err = rejected_statically(query);
            assert_eq!(
                err.to_string(),
                "InvalidArgumentType: DELETE requires a node or relationship, not an expression",
                "`{query}` keeps the generic message"
            );
        }
    }

    #[test]
    fn delete_backticked_keyword_variable_is_accepted() {
        // The backtick escape forces variable resolution (ADR-0280 ruling 1):
        // a bound `` `inf` `` is a legal DELETE target.
        parse_and_validate_expression_constraints("MATCH (`inf`) DELETE `inf`")
            .expect("backticked `inf` variable must be a valid DELETE target");
        parse_and_validate_expression_constraints(
            "MATCH (`nan`)-[r]->(`infinity`) DELETE r, `nan`, `infinity`",
        )
        .expect("backticked keyword variables must be valid DELETE targets");
    }

    // Neo4j `Finish.feature`: `DELETE null` is a legal no-op — null is the
    // one literal that can never be a non-deletable entity, and the executor
    // ignores `Value::Null`. The TCK's `DELETE 1 + 1` rejection (Delete5
    // scenario 9) is unaffected.
    #[test]
    fn delete_null_literal_is_accepted() {
        for query in ["DELETE null", "MATCH (n) DELETE null", "DELETE null FINISH"] {
            parse_and_validate_expression_constraints(query)
                .unwrap_or_else(|e| panic!("{query}: {e}"));
        }
    }

    // The one stated DELETE-target rule (check_delete_expr's doc, cited by
    // the executor's collect_delete): static validation walks the forms
    // that carry deletable slots — list elements, like CASE result
    // branches — and is permissive about computed forms (function calls,
    // subquery and comprehension expressions), leaving the runtime fold
    // the single authority on the resolved value.
    #[test]
    fn delete_target_rule_walks_list_elements_and_permits_computed_forms() {
        for query in [
            "MATCH (a), (b) DELETE [a, b]",
            "MATCH (a), (b) DETACH DELETE [a, null, [b]]",
            "MATCH (a)-[r]->(b) DELETE coalesce(a, r)",
            "MATCH (a) DELETE collect { MATCH (n) RETURN n }",
            "MATCH (a) DELETE [x IN [a] | x]",
        ] {
            parse_and_validate_expression_constraints(query)
                .unwrap_or_else(|e| panic!("`{query}` must admit statically: {e}"));
        }
        // A list element is a delete target in its own right: a statically
        // certain non-deletable element fails the whole statement here,
        // while a computed element stays permissive for the runtime fold.
        for query in ["MATCH (a) DELETE [a, 1 + 1]", "MATCH (a) DELETE [a, [1.5]]"] {
            rejected_statically(query);
        }
    }
}
