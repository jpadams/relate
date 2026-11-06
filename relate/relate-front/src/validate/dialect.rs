//! Dialect gates: compile-time rejection of language features the
//! statement's `CYPHER <version>` preamble (CIP-150, typed as
//! `Statement::cypher_version`) does not have.
//!
//! - [`validate_cypher5_restrictions`]: under `CYPHER 5`, reject the
//!   Cypher-25-only surface -- CIP-214's `vector()` value function, the
//!   CIP-216 `VECTOR<...>` type (property type constraints and `IS ::`
//!   predicates), CIP-224's `SEARCH` clause, CIP-235's vector-index `WITH
//!   [...]`/multi-label `FOR` forms.
//!
//! The gate is a typed AST walk -- never `format!("{stmt:?}")` string
//! matching, which cannot tell a `FunctionCall` node from a string literal
//! that merely *contains* the same text. It runs on the shared whole-tree
//! walker (`super::walk_statement_tree`), which recurses the whole
//! statement tree -- UNION arms, CIP-213 `NEXT` stages, `CALL { }` and
//! `WHEN`/`ELSE` bodies, `FOREACH` clause lists, and `EXISTS`/`COUNT`/
//! `COLLECT` expression subqueries -- so a gated feature cannot hide inside
//! a nested statement. Because a version preamble scopes the whole query
//! text, nested statements (which cannot carry their own preamble) are
//! checked against the enclosing statement's version, matching the
//! lowering's version threading over the NEXT chain
//! (`lower_statement_with_inherited_version`) and the executor's
//! `QueryCtx`.

use relate_core::is_cypher5;
use relate_core::temporal;
use relate_error::{EngineError, ErrorKind};
use relate_parser::{Clause, Expr, Statement, YieldMode};

use crate::ast::tree::walk_statement_tree;

/// CIP-150: reject Cypher-25-only features when the statement's preamble
/// explicitly selects `CYPHER 5` ([`relate_core::is_cypher5`]).
/// A statement with no explicit version, or an explicit `CYPHER 25`, is
/// completely unaffected.
pub fn validate_cypher5_restrictions(stmt: &Statement) -> Result<(), EngineError> {
    if !is_cypher5(stmt.cypher_version) {
        return Ok(());
    }
    walk_statement_tree(
        stmt,
        &mut |_| Ok(()),
        &mut cypher5_clause_check,
        &mut cypher5_expr_check,
    )
}

/// The per-clause structural arms of [`validate_cypher5_restrictions`]
/// (everything that is not expression-deep).
fn cypher5_clause_check(clause: &Clause) -> Result<(), EngineError> {
    match clause {
        Clause::Match(mc) => {
            // CIP-224: the `SEARCH ... IN (VECTOR INDEX ...)` clause.
            if mc.search.is_some() {
                return Err(EngineError::semantic(
                    ErrorKind::SyntaxError,
                    "UnsupportedLanguageFeature: the SEARCH clause is only available in Cypher 25"
                        .to_string(),
                ));
            }
        }
        Clause::CreateIndex(ci) => {
            // CIP-235: `CREATE VECTOR INDEX`'s `WITH [...]` additional-
            // properties clause and its multi-label/relationship-type
            // `FOR` form.
            if ci.kind == relate_parser::IndexKind::Vector {
                if ci.labels.len() > 1 {
                    return Err(EngineError::semantic(ErrorKind::SyntaxError,
                        "FeatureRemovedFromCypherVersion: multiple labels/relationship types on CREATE VECTOR INDEX are only available in Cypher 25".to_string(),
                    ));
                }
                if !ci.with_properties.is_empty() {
                    return Err(EngineError::semantic(ErrorKind::SyntaxError,
                        "FeatureRemovedFromCypherVersion: the WITH clause on CREATE VECTOR INDEX is only available in Cypher 25".to_string(),
                    ));
                }
            }
        }
        Clause::ShowConstraints(scc) => {
            // CIP-186/CIP-187: the graph-type columns are Cypher-25-only.
            // This whole-tree walk owns the version gate (a nested
            // `CALL { SHOW CONSTRAINTS YIELD classification }` is covered
            // too); the withheld set itself is one table in `show`.
            if scc.yields.mode() == YieldMode::Named {
                for name in scc.yields.names() {
                    if let Some(message) =
                        name.with_str(super::show::cypher5_withheld_constraint_column)
                    {
                        return Err(EngineError::semantic(
                            ErrorKind::SyntaxError,
                            message.to_string(),
                        ));
                    }
                }
            }
        }
        Clause::CreateConstraint(cc) => {
            // CIP-216: the `VECTOR<...>` property type constraint.
            if let Some(te) = &cc.property_type {
                if type_expr_contains_vector(te) {
                    return Err(EngineError::semantic(ErrorKind::SyntaxError,
                        "InvalidSyntax: Invalid input 'VECTOR', the vector type is only available in Cypher 25".to_string(),
                    ));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// The per-expression arm of [`validate_cypher5_restrictions`]: the
/// Cypher-25-only expression forms (the `vector()` function, the
/// `IS :: VECTOR` type predicate, and the CIP-228 two-argument temporal
/// constructor). Child descent is the walker's.
fn cypher5_expr_check(expr: &Expr) -> Result<(), EngineError> {
    match expr {
        Expr::FunctionCall(fc) => {
            if fc.name.with_str(|n| n.eq_ignore_ascii_case("vector")) {
                return Err(EngineError::semantic(
                    ErrorKind::UnknownFunction,
                    format!("unknown function '{}'", fc.name),
                ));
            }
            // The CIP-228 `(value, pattern)` form is Cypher-25-only; under
            // `CYPHER 5` the constructors keep their one-argument
            // signature, so every call with two or more arguments is
            // rejected here (a three-argument call would otherwise reach
            // the core arity rejection instead). The vocabulary (base
            // spellings plus aliases) is core's, not a second copy here.
            if fc.args.len() >= 2 && fc.name.with_str(temporal::accepts_pattern) {
                return Err(EngineError::semantic(
                    ErrorKind::SyntaxError,
                    format!(
                        "InvalidNumberOfArguments: {}() takes 1 argument under CYPHER 5; \
                         the pattern form is only available in Cypher 25",
                        fc.name
                    ),
                ));
            }
        }
        Expr::IsType(it) => {
            if type_expr_contains_vector(&it.type_expr) {
                return Err(EngineError::semantic(
                    ErrorKind::SyntaxError,
                    "InvalidSyntax: the VECTOR type predicate is only available in Cypher 25"
                        .to_string(),
                ));
            }
        }
        // CIP-261: interpolated string literals are Cypher-25-only syntax;
        // under CYPHER 5 they must be rejected like the rest of the
        // 25-only surface (round-5 review: the gate was missing).
        Expr::StringInterp(_) => {
            return Err(EngineError::semantic(
                ErrorKind::SyntaxError,
                "UnsupportedLanguageFeature: string interpolation (s'...') is only available in Cypher 25"
                    .to_string(),
            ));
        }
        _ => {}
    }
    Ok(())
}

/// True if a parsed `<value type>` is, or contains (inside a `LIST<...>` or
/// a closed union), the CIP-216 `VECTOR<...>` type.
fn type_expr_contains_vector(te: &relate_parser::TypeExpr) -> bool {
    use relate_parser::TypeExpr;
    match te {
        TypeExpr::Vector { .. } => true,
        TypeExpr::List { element, .. } => type_expr_contains_vector(element),
        TypeExpr::Union(members) => members.iter().any(type_expr_contains_vector),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    /// All tests go through the public lowering path so each gate is only
    /// green when the validator is actually wired into `lower_statement`.
    fn lower(query: &str) -> Result<(), String> {
        crate::parse_and_lower(query)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    // ---- String interpolation (Cypher-25-only, CIP-261) ----

    #[test]
    fn string_interpolation_is_rejected_under_cypher_5() {
        // Round-5 review: `s'...{expr}...'` is Cypher-25-only syntax and
        // must be gated like the vector() function and the VECTOR predicate.
        for query in [
            "CYPHER 5 RETURN s'a{1}b' AS r",
            "CYPHER 5 WITH 1 AS n RETURN s'{n}' AS r",
        ] {
            let err = lower(query).expect_err(&format!("`{query}` must be rejected"));
            assert!(
                err.contains("string interpolation"),
                "unexpected error for `{query}`: {err}"
            );
        }
        // Unprefixed and CYPHER 25 stay available (default dialect is 25).
        assert!(lower("RETURN s'a{1}b' AS r").is_ok());
        assert!(lower("CYPHER 25 RETURN s'a{1}b' AS r").is_ok());
    }

    // ---- Cypher-25-only features under `CYPHER 5` ----

    #[test]
    fn cypher5_rejects_search_clause() {
        let err = lower(
            "CYPHER 5 MATCH (n:Doc) SEARCH n IN (VECTOR INDEX docs FOR [1.0] LIMIT 2) RETURN n",
        )
        .expect_err("SEARCH must be rejected under CYPHER 5");
        assert!(err.contains("UnsupportedLanguageFeature"), "{err}");
        // The same query is fine under the default dialect.
        assert!(
            lower("MATCH (n:Doc) SEARCH n IN (VECTOR INDEX docs FOR [1.0] LIMIT 2) RETURN n")
                .is_ok()
        );
    }

    #[test]
    fn cypher5_rejects_vector_value_function() {
        let err = lower("CYPHER 5 RETURN vector([1.0], 2, FLOAT32) AS v")
            .expect_err("vector() must be rejected under CYPHER 5");
        assert!(err.contains("UnknownFunction"), "{err}");
        assert!(lower("CYPHER 25 RETURN vector([1.0], 2, FLOAT32) AS v").is_ok());
        // A string literal is not a call.
        assert!(lower("CYPHER 5 RETURN 'vector([1.0], 2, FLOAT32)' AS v").is_ok());
    }

    #[test]
    fn cypher5_rejects_vector_type_predicate_and_constraint() {
        let err =
            lower("CYPHER 5 CREATE CONSTRAINT c FOR (n:L) REQUIRE n.p IS :: VECTOR<FLOAT32>(2)")
                .expect_err("VECTOR property type must be rejected under CYPHER 5");
        assert!(err.contains("InvalidSyntax"), "{err}");
        assert!(
            lower("CREATE CONSTRAINT c FOR (n:L) REQUIRE n.p IS :: VECTOR<FLOAT32>(2)").is_ok()
        );
    }

    #[test]
    fn cypher5_rejects_vector_index_with_and_multi_label() {
        let err =
            lower("CYPHER 5 CREATE VECTOR INDEX docs FOR (n:Doc) ON n.embedding WITH [n.year]")
                .expect_err("WITH on a vector index must be rejected under CYPHER 5");
        assert!(err.contains("FeatureRemovedFromCypherVersion"), "{err}");
        let err = lower("CYPHER 5 CREATE VECTOR INDEX docs FOR (n:Doc|Article) ON n.embedding")
            .expect_err("multi-label vector index must be rejected under CYPHER 5");
        assert!(err.contains("FeatureRemovedFromCypherVersion"), "{err}");
        assert!(lower("CREATE VECTOR INDEX docs FOR (n:Doc) ON n.embedding WITH [n.year]").is_ok());
    }

    #[test]
    fn cypher5_rejects_show_constraints_classification_column() {
        let err = lower("CYPHER 5 SHOW CONSTRAINTS YIELD classification RETURN classification")
            .expect_err("classification must be rejected under CYPHER 5");
        assert!(err.contains("UnknownColumn"), "{err}");
        assert!(lower("SHOW CONSTRAINTS YIELD classification RETURN classification").is_ok());
    }

    #[test]
    fn cypher5_rejects_two_argument_temporal_constructors() {
        // CIP-228: the `(value, pattern)` form is Cypher-25-only; under
        // CYPHER 5 the one-argument signature stays.
        for query in [
            "CYPHER 5 RETURN datetime('2024-06-27T14:30:45.123+0200', \"yyyy-MM-dd'T'HH:mm:ss.SSSZ\") AS x",
            "CYPHER 5 RETURN localdatetime('2024-06-27T14:30:45.123', \"yyyy-MM-dd'T'HH:mm:ss.SSS\") AS x",
            "CYPHER 5 RETURN date('2024-06-27', 'yyyy-MM-dd') AS x",
            "CYPHER 5 RETURN time('14:30:45.123+0200', 'HH:mm:ss.SSSZ') AS x",
            "CYPHER 5 RETURN localtime('14:30:45.123', 'HH:mm:ss.SSS') AS x",
            "CYPHER 5 RETURN duration('1 hour', \"h 'hour'\") AS x",
            // The GQL alias spellings are the same constructors: the gate
            // reads core's one name table, aliases and all.
            "CYPHER 5 RETURN local_time('14:30:45', 'HH:mm:ss') AS x",
            "CYPHER 5 RETURN local_datetime('2024-06-27T14:30:45', \"yyyy-MM-dd'T'HH:mm:ss\") AS x",
            "CYPHER 5 RETURN zoned_datetime('2024-06-27T14:30:45+02:00', \"yyyy-MM-dd'T'HH:mm:ssXXX\") AS x",
            // One argument more than the Cypher 5 signature: the gate
            // rejects the whole call, not only the two-argument form.
            "CYPHER 5 RETURN date('2024-06-27', 'yyyy-MM-dd', 'x') AS x",
            "CYPHER 5 RETURN duration('P1D', 'y', 3) AS x",
        ] {
            let err = lower(query).expect_err(&format!("`{query}` must be rejected"));
            assert!(err.contains("InvalidNumberOfArguments"), "{query}: {err}");
        }
        // The one-argument forms, and the pattern form after an explicit
        // CYPHER 25, stay available.
        assert!(lower("CYPHER 5 RETURN date('2024-06-27') AS x").is_ok());
        assert!(lower("CYPHER 25 RETURN date('2024-06-27', 'yyyy-MM-dd') AS x").is_ok());
        assert!(lower(
            "RETURN datetime('2024-06-27T14:30:45.123+0200', \"yyyy-MM-dd'T'HH:mm:ss.SSSZ\") AS x"
        )
        .is_ok());
    }

    #[test]
    fn cypher5_gates_reach_nested_statements() {
        // The gates recurse into subquery bodies, so a gated feature cannot
        // hide from the preamble there.
        let err = lower("CYPHER 5 CALL { RETURN vector([1.0], 2, FLOAT32) AS v } RETURN v")
            .expect_err("vector() in a nested CALL must be rejected under CYPHER 5");
        assert!(err.contains("UnknownFunction"), "{err}");
        let err = lower("CYPHER 5 RETURN EXISTS { RETURN vector([1.0], 2, FLOAT32) AS v } AS e")
            .expect_err("vector() in an EXISTS body must be rejected under CYPHER 5");
        assert!(err.contains("UnknownFunction"), "{err}");
    }
}
