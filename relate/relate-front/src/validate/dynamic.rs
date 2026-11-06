//! LABS-709: compile-time validation of *statically known* dynamic
//! property-key and label/type operands (CIP-131 bracket targets,
//! CIP-156/CIP-162 dynamic labels/types).
//!
//! Neo4j folds constant dynamic operands at compile time, so a bad constant
//! fails even when the driving `MATCH` matches zero rows; Relate's checks
//! otherwise only run per row. The type and token-name rule itself lives in
//! `relate_core::dynamic` and is shared with the executor's runtime
//! resolution (`relate-executor/src/eval/labels.rs`), so neither layer
//! restates a message or a type rule.
//!
//! The AST descent is not re-derived here: the whole-tree walker
//! ([`crate::ast::tree::walk_statement_tree_with_operands`]) owns the
//! knowledge of where label expressions and write-site dynamic keys sit and
//! fires [`validate_operand`] at each of them, after the enclosing
//! statement's validation battery (so the historical first-error precedence
//! is unchanged).
//!
//! "Statically known" answers two questions. The *type* is decided from the
//! static type lattice: literals (list and map literals included), signed
//! literals, arithmetic — mixed Int/Float arithmetic and division included —
//! and comparisons/boolean operators, whose result types are fixed, plus the
//! fixed result type of a constant-argument constructor call
//! (`point({x: 3, y: 0})`). The *value* needs the narrower fold
//! ([`constant_value`], beside the lattice in [`super::static_types`]),
//! which is what the content rules (empty/NUL names) consume. An operand
//! that is only known at execution — a variable bound to runtime data, a
//! call over runtime data, an unbound parameter — stays runtime-checked
//! exactly as before, so a legitimately dynamic key/label keeps working.
//!
//! Two admission-time constants extend the fold's reach to what Neo4j also
//! treats as known before execution: a variable whose binding is itself
//! statically known (`WITH 1 + 2 AS a … SET n[a] = 1`) and a bound
//! parameter (`SET n[$a] = 1`). [`validate_scoped_operands`] walks the
//! clause list in order and types those operands against the bindings
//! visible at that point, so a bad one fails on a zero-row `MATCH` too. A
//! map literal is rejected by its type alone: the rule never reads (nor
//! fabricates) its entries, wherever the literal sits — at the operand or
//! as a list member.

use crate::ident_hash::{IdentHashMap, IdentHashSet};
use std::collections::BTreeMap;

use relate_core::{DynamicAccessError, DynamicNameToken, Value};
use relate_error::EngineError;
use relate_names::SmallName;
use relate_parser::{
    Clause, Expr, LabelExpr, PatternElement, PatternPath, PropertyAccess, SetItem, Statement,
};

use super::static_types::{constant_value, static_literal_type, FoldedConstant, StaticLitType};
use crate::ast::collect::{bind_pattern_vars, effective_name};
use crate::ast::tree::walk_label_expr;

/// A dynamic operand's statically known value or type. The value form is
/// checked by the shared core rule, so its exact type *and* list-element
/// types are the core's own; the named form is the type spelling for an
/// operand that is statically typed but has no materialized value (a map
/// literal, a mixed-numeric or boolean expression, a constructor call).
#[derive(Clone)]
enum StaticOperand {
    Value(Value),
    Named(&'static str),
}

/// One statement body's admission-time operand environment: the operand
/// types of variables bound by earlier clauses, and the statement's bound
/// parameters. Both are constants at admission, which is why Neo4j rejects a
/// bad dynamic operand of either form even when the driving `MATCH` matches
/// no rows — the per-row runtime check would never run.
#[derive(Clone)]
struct OperandEnv<'a> {
    vars: IdentHashMap<SmallName, StaticOperand>,
    params: Option<&'a BTreeMap<String, Value>>,
}

impl<'a> OperandEnv<'a> {
    /// The environment of a caller that has no bindings and no parameters:
    /// the whole-tree walk, which only sees the operand's own text.
    fn unbound() -> Self {
        Self {
            vars: IdentHashMap::default(),
            params: None,
        }
    }
}

/// The whole-tree walk's dynamic operand visit: reject an operand whose
/// statically known value the shared core rule would reject at runtime.
///
/// The operand's *value* is decided by the fold (literals, list and map
/// literals, constant arithmetic); its *type* by the static type lattice for
/// a constant expression the fold declines (mixed numeric arithmetic,
/// division, comparisons, boolean operators) and by the fixed result type of
/// a constant-argument constructor call (`point({x: 3, y: 0})`). The
/// *content* (empty / NUL names) needs a materialized value and is only
/// checked when one was produced, always through the shared core rule.
pub(crate) fn validate_operand(expr: &Expr, token: DynamicNameToken) -> Result<(), EngineError> {
    check_operand(expr, token, &OperandEnv::unbound())
}

/// Reject an operand from `env`'s statically known bindings: a variable
/// projected by an earlier clause or a bound parameter. Walks the clause
/// list in order and carries the operand types of the bindings made so far,
/// so a name rebound by a later `WITH`/`UNWIND`/`CALL` stops being typed at
/// exactly the point Neo4j's own binding does. Nested `CALL { }` and
/// `FOREACH` bodies are walked with their enclosing (correlated) bindings;
/// expression subqueries (`EXISTS`/`COUNT`/`COLLECT`) stay opaque leaves,
/// the same boundary `collect_expr_variables` draws.
pub(crate) fn validate_scoped_operands(
    stmt: &Statement,
    params: &BTreeMap<String, Value>,
) -> Result<(), EngineError> {
    let mut env = OperandEnv {
        vars: IdentHashMap::default(),
        params: Some(params),
    };
    walk_statement_operands(stmt, &mut env)
}

/// The statement positions the rule must reach: the statement's own clauses,
/// its UNION arms, its CIP-213 NEXT stages, and (through
/// [`walk_clause_operands`]) the WHEN/ELSE bodies of a top-level
/// `Conditional`. Every one of them is lowered and executed, so a write site
/// inside one must be admitted by the same rule — the clause-only walk
/// silently skipped them.
fn walk_statement_operands(stmt: &Statement, env: &mut OperandEnv<'_>) -> Result<(), EngineError> {
    walk_clause_operands(&stmt.clauses, env)?;
    // Each UNION arm is its own statement scope, seeing the head's bindings;
    // the arm's own `union` chain recurses here.
    if let Some(union) = stmt.union.as_deref() {
        let mut arm_env = env.clone();
        walk_statement_operands(&union.statement, &mut arm_env)?;
    }
    // A NEXT stage sees the previous stage's bindings.
    if let Some(next) = &stmt.next {
        let mut next_env = env.clone();
        walk_statement_operands(next, &mut next_env)?;
    }
    Ok(())
}

fn check_operand(
    expr: &Expr,
    token: DynamicNameToken,
    env: &OperandEnv<'_>,
) -> Result<(), EngineError> {
    match static_operand(expr, env) {
        Some(StaticOperand::Value(value)) => token.check(&value).map_err(dynamic_access_error),
        Some(StaticOperand::Named(type_name)) => {
            if accepts_type(token, type_name) {
                Ok(())
            } else {
                Err(dynamic_access_error(DynamicAccessError::wrong_type(
                    token, type_name,
                )))
            }
        }
        // Unknown at admission: left to the executor's per-row rule, so a
        // legitimately dynamic key/label keeps working.
        None => Ok(()),
    }
}

/// An expression's statically known operand value or type, resolved through
/// `env` for a variable or parameter and from the expression itself
/// otherwise.
fn static_operand(expr: &Expr, env: &OperandEnv<'_>) -> Option<StaticOperand> {
    match expr {
        Expr::Paren(inner) => static_operand(inner, env),
        Expr::Variable(name) => env.vars.get(name).cloned(),
        Expr::Parameter(name) => env
            .params
            .and_then(|params| params.get(name.as_str()))
            .map(|value| StaticOperand::Value(value.clone())),
        _ => match constant_value(expr) {
            Some(FoldedConstant::Value(value)) => Some(StaticOperand::Value(value)),
            Some(FoldedConstant::Map) => Some(StaticOperand::Named(StaticLitType::Map.name())),
            None => static_literal_type(expr)
                .map(|ty| StaticOperand::Named(ty.name()))
                .or_else(|| constant_function_type_name(expr).map(StaticOperand::Named)),
        },
    }
}

/// The statically-known operand types each namespace accepts: the *type*
/// half of the core rule ([`DynamicNameToken::expected`] is its authored
/// text, [`DynamicNameToken::check`] its per-value/content half). A property
/// key is a `STRING`; a dynamic label/type is a `STRING` or a `LIST` whose
/// elements the core rule then checks. An operand with no statically known
/// type never reaches here, so a runtime-bound key/label is unaffected.
fn accepts_type(token: DynamicNameToken, type_name: &str) -> bool {
    match token {
        DynamicNameToken::PropertyKey => type_name == StaticLitType::String.name(),
        DynamicNameToken::Label => {
            type_name == StaticLitType::String.name() || type_name == StaticLitType::List.name()
        }
    }
}

/// The result type of a built-in function whose return type is fixed by the
/// function itself, independent of its argument types. Only functions whose
/// fixed result is *not* `STRING` are listed: a fixed `STRING` result can
/// never violate the dynamic-name rule, so typing it would add a table row
/// with no effect. The spellings mirror the core type vocabulary's
/// (`relate_core::typename::bare_type_name`), which is what the runtime
/// channel renders for a folded value.
fn fixed_function_type_name(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "date" => "DATE",
        "localtime" => "LOCAL TIME",
        "time" => "ZONED TIME",
        "localdatetime" => "LOCAL DATETIME",
        "datetime" => "ZONED DATETIME",
        "duration" => "DURATION",
        "point" => "POINT",
        "tointeger" | "tointegerornull" => "INTEGER",
        "tofloat" | "tofloatornull" => "FLOAT",
        "toboolean" | "tobooleanornull" => "BOOLEAN",
        "size" | "length" => "INTEGER",
        "rand" => "FLOAT",
        _ => return None,
    })
}

/// The result type of a call whose arguments are all statically known, or
/// `None` when the call's result is not statically decided. Only
/// constant-argument calls are typed: a call over runtime data keeps its
/// per-row semantics and its own argument-type errors (`point(n.p)` reports
/// the bad argument, not a bad operand, exactly as Neo4j does).
fn constant_function_type_name(expr: &Expr) -> Option<&'static str> {
    let Expr::FunctionCall(call) = expr else {
        return None;
    };
    let name = call.name.with_str(fixed_function_type_name)?;
    call.args
        .iter()
        .all(|arg| constant_value(arg).is_some() || static_literal_type(arg).is_some())
        .then_some(name)
}

fn dynamic_access_error(error: DynamicAccessError) -> EngineError {
    EngineError::semantic(error.kind(), error.to_string())
}

/// The clause-ordered half of [`validate_scoped_operands`]: check each
/// clause's pattern and write-site dynamic operands against the bindings
/// visible at that point, then extend (or drop) the bindings the clause
/// itself makes.
fn walk_clause_operands(clauses: &[Clause], env: &mut OperandEnv<'_>) -> Result<(), EngineError> {
    for clause in clauses {
        match clause {
            Clause::Match(mc) => {
                for path in &mc.pattern {
                    check_path_operands(path, env)?;
                }
                // A pattern's own variables are entities (or fresh), so
                // their operand types are unknown from here on.
                rebind_paths(&mc.pattern, env);
            }
            Clause::Create(cc) => {
                for path in &cc.pattern {
                    check_path_operands(path, env)?;
                }
                rebind_paths(&cc.pattern, env);
            }
            Clause::Merge(mc) => {
                check_path_operands(&mc.pattern, env)?;
                for item in mc.on_create.iter().chain(&mc.on_match) {
                    check_set_item_operands(item, env)?;
                }
                rebind_paths(std::slice::from_ref(&mc.pattern), env);
            }
            Clause::Set(sc) => {
                for item in &sc.items {
                    check_set_item_operands(item, env)?;
                }
            }
            Clause::Remove(rc) => {
                for item in &rc.items {
                    check_property_key(item.property.as_ref(), env)?;
                    for label in &item.labels {
                        check_label_operands(label, env)?;
                    }
                }
            }
            Clause::With(wc) => {
                // `WITH` projects against the pre-clause bindings; the
                // aliases it introduces replace the scope wholesale.
                let mut next = IdentHashMap::default();
                for item in &wc.items {
                    if item.all {
                        next.extend(env.vars.clone());
                        continue;
                    }
                    let Some(alias) = effective_name(item) else {
                        continue;
                    };
                    if let Some(operand) = static_operand(&item.expr, env) {
                        next.insert(alias, operand);
                    }
                }
                env.vars = next;
            }
            Clause::Unwind(uw) => {
                env.vars.remove(&uw.alias);
            }
            Clause::LoadCSV(lc) => {
                env.vars.remove(&lc.alias);
            }
            Clause::CallProcedure(cp) => {
                for name in cp.yields.effective_names() {
                    env.vars.remove(&name);
                }
            }
            Clause::CallSubquery(csq) => {
                let mut inner = env.clone();
                // The whole nested statement, not just its own clauses: a
                // CALL body can carry UNION arms and NEXT stages, whose write
                // sites must be admitted by the same rule.
                walk_statement_operands(&csq.statement, &mut inner)?;
                // Only names the subquery newly binds can escape into the
                // enclosing scope; a name already bound there would be a
                // `VariableAlreadyBound` error, checked by the scope
                // validator.
                for (name, operand) in inner.vars {
                    env.vars.entry(name).or_insert(operand);
                }
            }
            Clause::Foreach(f) => {
                let mut inner = env.clone();
                inner.vars.remove(&f.variable);
                walk_clause_operands(&f.clauses, &mut inner)?;
            }
            // CIP-135 `WHEN … THEN <statement>` / `ELSE <statement>`:
            // executed subquery positions, walked with the enclosing
            // bindings.
            Clause::Conditional(conditional) => {
                for branch in &conditional.branches {
                    let mut inner = env.clone();
                    walk_statement_operands(&branch.body, &mut inner)?;
                }
                if let Some(body) = &conditional.else_body {
                    let mut inner = env.clone();
                    walk_statement_operands(body, &mut inner)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn check_set_item_operands(item: &SetItem, env: &OperandEnv<'_>) -> Result<(), EngineError> {
    check_property_key(item.property.as_ref(), env)?;
    for label in &item.labels {
        check_label_operands(label, env)?;
    }
    // `SET labels(n) = <expr>` resolves its value through the dynamic-label
    // rule, exactly like a dynamic label atom.
    if item.labels_function.is_some() {
        if let Some(value) = &item.value {
            check_operand(value, DynamicNameToken::Label, env)?;
        }
    }
    Ok(())
}

fn check_property_key(
    property: Option<&PropertyAccess>,
    env: &OperandEnv<'_>,
) -> Result<(), EngineError> {
    let Some(key) = property.and_then(|p| p.key_expr.as_ref()) else {
        return Ok(());
    };
    check_operand(key, DynamicNameToken::PropertyKey, env)
}

fn check_label_operands(label: &LabelExpr, env: &OperandEnv<'_>) -> Result<(), EngineError> {
    // The leaf rule only: the shared walker ([`walk_label_expr`]) owns the
    // `Not`/`And`/`Or` fold.
    walk_label_expr(label, &mut |le| match le {
        LabelExpr::Dynamic(d) => check_operand(&d.expr, DynamicNameToken::Label, env),
        _ => Ok(()),
    })
}

fn check_path_operands(path: &PatternPath, env: &OperandEnv<'_>) -> Result<(), EngineError> {
    for element in &path.elements {
        check_element_operands(element, env)?;
    }
    Ok(())
}

fn check_element_operands(
    element: &PatternElement,
    env: &OperandEnv<'_>,
) -> Result<(), EngineError> {
    match element {
        PatternElement::Node(node) => {
            if let Some(label) = &node.label_expr {
                check_label_operands(label, env)?;
            }
        }
        PatternElement::Relationship(rel) => {
            if let Some(ty) = &rel.type_expr {
                check_label_operands(ty, env)?;
            }
        }
        PatternElement::Quantified(quantified) => {
            for inner in &quantified.elements {
                check_element_operands(inner, env)?;
            }
        }
    }
    Ok(())
}

/// Drop the operand types of every variable a pattern binds.
fn rebind_paths(paths: &[PatternPath], env: &mut OperandEnv<'_>) {
    let mut bound = IdentHashSet::default();
    bind_pattern_vars(paths, &mut bound);
    for name in bound {
        env.vars.remove(&name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::validate_statement_tree;

    /// Parse `query` and run the whole nested-statement validation battery.
    fn check(query: &str) -> Result<(), EngineError> {
        validate_statement_tree(&relate_parser::parse(query).expect("parse"))
    }

    fn error(query: &str) -> EngineError {
        check(query).expect_err(query)
    }

    fn assert_error(query: &str, needles: &[&str]) {
        let error = error(query);
        let rendered = error.to_string();
        for needle in needles {
            assert!(
                rendered.contains(needle),
                "`{query}`: expected {needle:?} in {rendered:?}"
            );
        }
    }

    fn assert_ok(query: &str) {
        check(query).unwrap_or_else(|error| panic!("`{query}`: {error}"));
    }

    #[test]
    fn constant_wrong_type_property_keys_fail_at_compile_time() {
        for query in [
            "MATCH (n) SET n[1] = 2",
            "MATCH (n) SET n[true] = 2",
            "MATCH (n) SET n[1.5] = 2",
            "MATCH (n) SET n[null] = 2",
            "MATCH (n) SET n[['a']] = 2",
            "MATCH (n) REMOVE n[1]",
            "MATCH (n) SET n[1 + 1] = 2",
        ] {
            assert_error(
                query,
                &[
                    "node or relationship property key",
                    "Expected to be STRING.",
                ],
            );
        }
    }

    /// The detail text reads the one core type spelling, so a union keeps
    /// the CIP-100 order (`INTEGER` before `FLOAT`) rather than the
    /// lexicographic order a third table would produce.
    #[test]
    fn list_property_key_detail_uses_the_cip100_type_order() {
        assert_error(
            "MATCH (n) SET n[[1, 1.5]] = 2",
            &["Invalid input 'LIST<INTEGER | FLOAT>' for node or relationship property key"],
        );
    }

    /// The family's core regression: Neo4j folds the constant at compile
    /// time, so the zero-row MATCH never hides it. Relate's per-row check
    /// would never run here.
    #[test]
    fn constant_wrong_type_property_key_fails_on_a_zero_row_match() {
        assert_error(
            "MATCH (n:NoSuchLabel) SET n[1] = 2",
            &["node or relationship property key", "INTEGER"],
        );
    }

    #[test]
    fn constant_empty_and_nul_property_keys_fail_at_compile_time() {
        for query in [
            "MATCH (n) SET n[''] = 2",
            "MATCH (n) SET n['a\u{0}b'] = 2",
            "MATCH (n) REMOVE n['']",
        ] {
            assert_error(
                query,
                &["invalid name. A property key name cannot be empty or contain any null-bytes"],
            );
        }
    }

    #[test]
    fn constant_dynamic_labels_must_be_string_or_string_list() {
        for query in [
            "MATCH (n) SET n:$(1)",
            "MATCH (n) SET n:$(['A', 1])",
            "MATCH (n) REMOVE n:$(true)",
            "MATCH (n) CREATE (m:$(1))",
            "MATCH (n) MERGE (m:$(1))",
            "MATCH (n:$(1)) RETURN n",
            "MATCH (n) WHERE n:$(1) RETURN n",
            "MATCH (n) SET labels(n) = 1",
            "MATCH (n) SET labels(n) = ['A', 1]",
            "MATCH (n) WHERE (n)-[:$(1)]->() RETURN n",
            "MATCH (n) RETURN [(n)-[:$(1)]->(m) | m] AS ms",
            "MATCH (a), (b) RETURN shortestPath((a)-[:$(1)]->(b)) AS p",
        ] {
            assert_error(
                query,
                &["dynamic label", "Expected to be STRING or LIST<STRING>."],
            );
        }
    }

    #[test]
    fn constant_dynamic_relationship_types_are_checked_too() {
        assert_error(
            "CREATE (:A)-[:$(1)]->(:B)",
            &["dynamic label", "Expected to be STRING or LIST<STRING>."],
        );
    }

    #[test]
    fn constant_empty_and_nul_labels_fail_at_compile_time() {
        for query in [
            "MATCH (n) SET n:$('')",
            "MATCH (n) SET n:$(['A', ''])",
            "MATCH (n) SET n:$('a\u{0}b')",
        ] {
            assert_error(
                query,
                &["invalid name. A label name cannot be empty or contain any null-bytes"],
            );
        }
    }

    /// A map literal is a MAP by shape, whatever its entries evaluate to;
    /// the fold rejects it without reading (or fabricating) a value.
    #[test]
    fn constant_map_operands_are_rejected_by_shape() {
        for query in [
            "MATCH (n) SET n[{a: 1}] = 2",
            "MATCH (n) SET n[({a: $p})] = 2",
        ] {
            assert_error(
                query,
                &["Invalid input 'MAP' for node or relationship property key"],
            );
        }
        assert_error(
            "MATCH (n) SET n:$({a: 1})",
            &["Invalid input 'MAP' for dynamic label"],
        );
    }

    /// A map literal nested inside a constant list is declined by the value
    /// fold (a list cannot materialize a map member) but is still
    /// constant-typed: it must fail through the type half — at the operand or
    /// as a list member, on a zero-row MATCH too.
    #[test]
    fn map_literals_nested_in_constant_lists_are_rejected_by_shape() {
        for query in [
            "MATCH (n) SET n[[{a: 1}]] = 2",
            "MATCH (n:NoSuchLabel) SET n[[{a: $p}]] = 2",
            "MATCH (n:NoSuchLabel) REMOVE n[[{a: 1}]]",
        ] {
            assert_error(
                query,
                &["Invalid input 'MAP' for node or relationship property key"],
            );
        }
        for query in [
            "MATCH (n) SET n:$(['A', {b: 1}])",
            "MATCH (n:NoSuchLabel) SET n:$(['A', {b: $p}])",
            "MATCH (n) WHERE n:$([{b: 1}]) RETURN n",
            "MATCH (n) SET labels(n) = [{b: 1}]",
        ] {
            assert_error(query, &["Invalid input 'MAP' for dynamic label"]);
        }
    }

    /// An operand whose constant expression the value fold declines for
    /// content reasons still has a decided *type*, so the zero-row MATCH that
    /// would never evaluate it cannot hide it.
    #[test]
    fn statically_typed_non_string_operands_fail_at_compile_time() {
        for (query, type_name) in [
            ("MATCH (n:NoSuchLabel) SET n[1 + 1.5] = 2", "FLOAT"),
            ("MATCH (n:NoSuchLabel) SET n[2 / 1] = 2", "INTEGER"),
            ("MATCH (n:NoSuchLabel) SET n[1 < 2] = 2", "BOOLEAN"),
            ("MATCH (n:NoSuchLabel) SET n[NOT true] = 2", "BOOLEAN"),
        ] {
            let needle =
                format!("Invalid input '{type_name}' for node or relationship property key");
            assert_error(query, &[needle.as_str()]);
        }
        for (query, type_name) in [
            ("MATCH (n) SET n:$(1 + 1.5)", "FLOAT"),
            ("MATCH (n) SET n:$(1 < 2)", "BOOLEAN"),
        ] {
            let needle = format!("Invalid input '{type_name}' for dynamic label");
            assert_error(query, &[needle.as_str()]);
        }
    }

    #[test]
    fn constant_string_keys_and_labels_are_accepted() {
        for query in [
            "MATCH (n) SET n['name'] = 2",
            "MATCH (n) SET n['na' + 'me'] = 2",
            "MATCH (n) REMOVE n['name']",
            "MATCH (n) SET n:$('Person')",
            "MATCH (n) SET n:$(['Person', 'Admin'])",
            "MATCH (n) CREATE (m:$('Person'))",
        ] {
            assert_ok(query);
        }
    }

    /// Parameters and runtime-computed expressions are not constant-foldable
    /// here, so they stay runtime-checked: the compile-time pass must not
    /// reject a legitimately dynamic key/label (no bound values were
    /// supplied, so neither the fold nor the parameter rule has anything to
    /// decide).
    #[test]
    fn parameters_and_runtime_expressions_are_left_to_runtime() {
        for query in [
            "MATCH (n) SET n[$key] = 2",
            "MATCH (n) SET n[toString(n.x)] = 2",
            "MATCH (n) SET n:$(label)",
            "MATCH (n) SET n:$($labels)",
            "MATCH (n) WHERE n:$(label) RETURN n",
        ] {
            assert_ok(query);
        }
    }

    /// Parse, run the whole-tree battery, then the binding/parameter pass —
    /// the same order lowering uses.
    fn admit(query: &str, params: &[(&str, Value)]) -> Result<(), EngineError> {
        let stmt = relate_parser::parse(query).expect("parse");
        let params: BTreeMap<String, Value> = params
            .iter()
            .map(|(name, value)| ((*name).to_string(), value.clone()))
            .collect();
        validate_statement_tree(&stmt)?;
        validate_scoped_operands(&stmt, &params)
    }

    fn assert_admit_error(query: &str, params: &[(&str, Value)], needles: &[&str]) {
        let rendered = admit(query, params).expect_err(query).to_string();
        for needle in needles {
            assert!(
                rendered.contains(needle),
                "`{query}`: expected {needle:?} in {rendered:?}"
            );
        }
    }

    fn assert_admit_ok(query: &str, params: &[(&str, Value)]) {
        admit(query, params).unwrap_or_else(|error| panic!("`{query}`: {error}"));
    }

    /// A constructor call over constant arguments is folded at admission by
    /// Neo4j, so its fixed result type must be rejected even on a zero-row
    /// `MATCH`: these are the shapes that used to slip through because the
    /// fold decline on a function result was read as "not statically known".
    #[test]
    fn constant_constructor_calls_are_typed_for_property_keys() {
        for (query, type_name) in [
            (
                "MATCH (n:NoSuchLabel) SET n[localdatetime(\"2024185T19:32:24\")] = 1",
                "LOCAL DATETIME",
            ),
            ("MATCH (n:NoSuchLabel) SET n[point({x:3,y:0})] = 1", "POINT"),
            (
                "MATCH (n:NoSuchLabel) SET n[date(\"2020-01-01\")] = 1",
                "DATE",
            ),
            (
                "MATCH (n:NoSuchLabel) SET n[duration(\"P1D\")] = 1",
                "DURATION",
            ),
            (
                "MATCH (n:NoSuchLabel) SET n[toInteger(\"12\")] = 1",
                "INTEGER",
            ),
            ("MATCH (n:NoSuchLabel) SET n[size(\"ab\")] = 1", "INTEGER"),
            ("MATCH (n:NoSuchLabel) SET n[rand()] = 1", "FLOAT"),
        ] {
            let needle =
                format!("Invalid input '{type_name}' for node or relationship property key");
            assert_error(query, &[needle.as_str()]);
        }
        for (query, type_name) in [
            (
                "MATCH (n:NoSuchLabel) REMOVE n[localdatetime(\"2024185T19:32:24\")]",
                "LOCAL DATETIME",
            ),
            ("MATCH (n:NoSuchLabel) REMOVE n[point({x:3,y:0})]", "POINT"),
        ] {
            let needle =
                format!("Invalid input '{type_name}' for node or relationship property key");
            assert_error(query, &[needle.as_str()]);
        }
    }

    #[test]
    fn constant_constructor_calls_are_typed_for_labels() {
        for (query, type_name) in [
            (
                "MATCH (n:NoSuchLabel) SET n:$(localdatetime(\"2024185T19:32:24\"))",
                "LOCAL DATETIME",
            ),
            ("MATCH (n:NoSuchLabel) SET n:$(point({x:3,y:0}))", "POINT"),
            (
                "MATCH (n:NoSuchLabel) REMOVE n:$(point({x:3,y:0}))",
                "POINT",
            ),
            (
                "MATCH (n:NoSuchLabel) REMOVE n:$(localdatetime(\"2024185T19:32:24\"))",
                "LOCAL DATETIME",
            ),
        ] {
            let needle = format!("Invalid input '{type_name}' for dynamic label");
            assert_error(query, &[needle.as_str()]);
        }
    }

    /// A call over runtime data keeps its per-row semantics: the type is not
    /// statically decided, and Neo4j reports the call's own argument error
    /// there rather than a dynamic-operand error.
    #[test]
    fn calls_over_runtime_data_stay_runtime() {
        for query in [
            "MATCH (n) SET n[point(n.p)] = 1",
            "MATCH (n) SET n[localdatetime(n.p)] = 1",
            "MATCH (n) SET n[toString(n.p)] = 1",
        ] {
            assert_ok(query);
        }
    }

    /// `WITH <expr> AS a … SET n[a] = 1`: the operand is a variable, but its
    /// binding is statically known, and Neo4j rejects on a zero-row MATCH.
    #[test]
    fn with_bound_variables_are_typed_for_property_keys() {
        for (binding, type_name) in [
            ("1 + 2", "INTEGER"),
            ("true", "BOOLEAN"),
            ("point({x: 2.3, y: 4.5})", "POINT"),
            ("localdatetime(\"2024185T19:32:24\")", "LOCAL DATETIME"),
            ("{x : 1}", "MAP"),
        ] {
            let needle =
                format!("Invalid input '{type_name}' for node or relationship property key");
            for query in [
                format!("WITH {binding} AS a MATCH (n) SET n[a] = 1 RETURN n"),
                format!("WITH {binding} AS a MATCH (n) REMOVE n[a] RETURN n"),
            ] {
                assert_admit_error(&query, &[], &[needle.as_str()]);
            }
        }
    }

    #[test]
    fn with_bound_variables_are_typed_for_labels() {
        for (binding, type_name) in [
            ("1 + 2", "INTEGER"),
            ("true", "BOOLEAN"),
            ("point({x:3,y:0})", "POINT"),
            ("localdatetime(\"2024185T19:32:24\")", "LOCAL DATETIME"),
        ] {
            let needle = format!("Invalid input '{type_name}' for dynamic label");
            let query = format!("WITH {binding} AS a MATCH (n) REMOVE n:$(a) RETURN labels(n)");
            assert_admit_error(&query, &[], &[needle.as_str()]);
        }
    }

    /// A list binding is checked as a *value*, so the shared core rule runs
    /// over its elements: the key side names the whole list type, the label
    /// side names the offending element (the core rule's documented
    /// per-element detail).
    #[test]
    fn with_bound_list_variables_are_checked_by_value() {
        assert_admit_error(
            "WITH [1, 2] AS a MATCH (n) SET n[a] = 1 RETURN n",
            &[],
            &["Invalid input 'LIST<INTEGER>' for node or relationship property key"],
        );
        assert_admit_error(
            "WITH [1, 2] AS a MATCH (n) REMOVE n:$(a) RETURN labels(n)",
            &[],
            &["Invalid input 'INTEGER' for dynamic label"],
        );
    }

    #[test]
    fn bound_parameters_are_typed() {
        for (value, type_name) in [
            (Value::Int(1), "INTEGER"),
            (Value::Bool(true), "BOOLEAN"),
            (Value::Float(1.5), "FLOAT"),
            (map_param(), "MAP"),
        ] {
            let needle =
                format!("Invalid input '{type_name}' for node or relationship property key");
            for query in [
                "MATCH (n) SET n[$a] = 1 RETURN n",
                "MATCH (n) REMOVE n[$a] RETURN n",
            ] {
                assert_admit_error(query, &[("a", value.clone())], &[needle.as_str()]);
            }
        }
        for (value, type_name) in [(Value::Int(1), "INTEGER"), (map_param(), "MAP")] {
            let needle = format!("Invalid input '{type_name}' for dynamic label");
            for query in [
                "MATCH (n) SET n:$($a) RETURN labels(n)",
                "MATCH (n) REMOVE n:$($a) RETURN labels(n)",
            ] {
                assert_admit_error(query, &[("a", value.clone())], &[needle.as_str()]);
            }
        }
    }

    fn map_param() -> Value {
        Value::Map(std::sync::Arc::new(BTreeMap::from([(
            "x".to_string(),
            Value::Int(1),
        )])))
    }

    /// Only a *bad* binding is rejected: string bindings and bindings whose
    /// value is only known at execution keep working, and a name rebound by
    /// a later clause is typed by its new binding, not the old one.
    #[test]
    fn good_and_rebound_bindings_stay_accepted() {
        for query in [
            "WITH 'key' AS a MATCH (n) SET n[a] = 1 RETURN n",
            "WITH 'Label' AS a MATCH (n) REMOVE n:$(a) RETURN labels(n)",
            "MATCH (n) WITH n.name AS a SET n[a] = 1 RETURN n",
            "WITH 1 AS a WITH 'key' AS a MATCH (n) SET n[a] = 1 RETURN n",
            "WITH 1 AS a MATCH (n) WITH 'key' AS a SET n[a] = 1 RETURN n",
            "MATCH (n) UNWIND [1] AS a SET n[a] = 1 RETURN n",
        ] {
            assert_admit_ok(query, &[]);
        }
        assert_admit_ok(
            "MATCH (n) SET n[$a] = 1 RETURN n",
            &[("a", Value::String("key".into()))],
        );
        assert_admit_ok(
            "MATCH (n) SET n:$($a) RETURN labels(n)",
            &[("a", Value::String("Label".into()))],
        );
        // A parameter that is not bound at admission has nothing to decide.
        assert_admit_ok("MATCH (n) SET n[$a] = 1 RETURN n", &[]);
    }

    /// The scoped pass walks `FOREACH` bodies with the enclosing bindings,
    /// so the same rule reaches a nested write site.
    #[test]
    fn scoped_bindings_reach_foreach_bodies() {
        assert_admit_error(
            "WITH 1 AS a MATCH (n) FOREACH (x IN [1] | SET n[a] = 1) RETURN n",
            &[],
            &["Invalid input 'INTEGER' for node or relationship property key"],
        );
    }

    /// Every statement position the executor reaches is walked, not just the
    /// head statement's own clauses: a write site inside a UNION arm, a
    /// CIP-213 NEXT stage, or a CIP-135 WHEN/ELSE body is admitted by the
    /// same rule. The scoped walk is called directly so the enclosing
    /// statement battery (union-column and scope rules) does not pre-empt
    /// the error under test.
    #[test]
    fn scoped_bindings_reach_union_next_and_conditional_positions() {
        fn scoped_error(query: &str) -> EngineError {
            let stmt = relate_parser::parse(query).expect("parse");
            validate_scoped_operands(&stmt, &BTreeMap::new()).expect_err(query)
        }
        for query in [
            "WITH 1 AS a MATCH (n) RETURN n AS x UNION MATCH (m) SET m[a] = 1 RETURN m AS x",
            "WITH 1 AS a MATCH (n) RETURN n AS x NEXT MATCH (m) SET m[a] = 1 RETURN m AS x",
            "WHEN true THEN WITH 1 AS a MATCH (n) SET n[a] = 1 RETURN n AS x ELSE RETURN 1 AS x",
        ] {
            let rendered = scoped_error(query).to_string();
            assert!(
                rendered.contains("Invalid input 'INTEGER' for node or relationship property key"),
                "`{query}`: {rendered}"
            );
        }
    }
}
