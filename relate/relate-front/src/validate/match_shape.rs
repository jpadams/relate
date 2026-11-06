//! Match-shape validation: the path-selector, REPEATABLE ELEMENTS,
//! quantified-path-pattern, shortest-path and update-clause pattern rules
//! Neo4j 2026 enforces at compile time but Relate accepted (the V1
//! harness wave's MATCH-shape cluster).
//!
//! Five rules live here:
//!
//! - **42N34** — a path variable cannot be bound inside a quantified path
//!   pattern (`MATCH (p = (:A)--(:B))+`). CIP-28 permits the spelling; the
//!   live engine rejects it.
//! - **42I21** — a parenthesized/quantified path pattern's `WHERE` may not
//!   reference the enclosing path variable (`MATCH p = ((:A)-->+(:B) WHERE
//!   length(p) …)`), nor — under `REPEATABLE ELEMENTS`, for a selective path
//!   pattern — a variable declared by a sibling path pattern of the same
//!   graph pattern.
//! - **42N59** — under `REPEATABLE ELEMENTS`, a strict interior variable of
//!   a selective path pattern (any node/relationship variable that is not one
//!   of the path's two endpoint nodes) may not be redeclared by another path
//!   pattern of the same graph pattern.
//! - **42N56** — a `shortestPath()`/`allShortestPaths()` pattern supports no
//!   relationship properties.
//! - **42I04** — a `CREATE`/`MERGE` pattern admits neither a
//!   `shortestPath()`/`allShortestPaths()` pattern nor an explicit path mode.
//!
//! The parser owns the shape rules that must fire before the AST loses the
//! information (a named group under a non-selective selector, and a path /
//! sub-path variable sharing one name); see `parser::patterns`. The
//! `USING INDEX` hint rules are a separate MATCH-shape family and live in
//! [`super::index_hint`].

use crate::ident_hash::IdentHashSet;
use relate_names::SmallName;

use relate_error::{EngineError, ErrorKind};
use relate_parser::{
    selector_is_selective, Clause, MatchClause, MatchMode, PatternElement, PatternPath,
};

use crate::ast::collect::collect_expr_variables;

/// The `MATCH`-shape rule battery, one pass per `MATCH` clause.
pub fn validate_match_shape(clauses: &[Clause]) -> Result<(), EngineError> {
    for clause in clauses {
        let Some(patterns) = crate::ast::collect::clause_patterns(clause) else {
            continue;
        };
        match clause {
            Clause::Match(mc) => {
                for path in patterns {
                    reject_path_bound_in_qpp(path)?;
                    reject_own_path_variable_in_group_where(path)?;
                    reject_shortest_path_properties(path)?;
                }
                if mc.match_mode == MatchMode::RepeatableElements {
                    reject_repeatable_elements_violations(mc)?;
                }
            }
            // 42I04: the update clauses accept neither a `shortestPath()` /
            // `allShortestPaths()` pattern nor an explicit path mode.
            Clause::Create(_) => reject_update_path_shape("CREATE", patterns)?,
            Clause::Merge(_) => reject_update_path_shape("MERGE", patterns)?,
            _ => {}
        }
    }
    Ok(())
}

/// 42I04: `shortestPath()`/`allShortestPaths()` and explicit path modes are
/// MATCH-only pattern shapes — Neo4j rejects them in a `CREATE`/`MERGE`
/// pattern ("'…' cannot be used in a MERGE clause").
fn reject_update_path_shape(clause: &str, patterns: &[PatternPath]) -> Result<(), EngineError> {
    for path in patterns {
        if path.is_shortest_path_function {
            return Err(EngineError::semantic(
                ErrorKind::InvalidPatternCharacter,
                format!(
                    "invalid expression: 'shortestPath()/allShortestPaths()' cannot be used in a {clause} clause"
                ),
            ));
        }
        if let Some(mode) = &path.mode {
            return Err(EngineError::semantic(
                ErrorKind::InvalidPatternCharacter,
                format!(
                    "invalid expression: '{}' cannot be used in a {clause} clause",
                    path_mode_name(mode)
                ),
            ));
        }
    }
    Ok(())
}

/// The path-mode keyword as written, for the 42I04 description text.
fn path_mode_name(mode: &relate_parser::PathMode) -> &'static str {
    use relate_parser::PathMode;
    match mode {
        PathMode::Walk => "WALK",
        PathMode::Trail => "TRAIL",
        PathMode::Acyclic => "ACYCLIC",
        PathMode::Simple => "SIMPLE",
    }
}

/// 42N56: `shortestPath()`/`allShortestPaths()` patterns support no
/// relationship properties ("Properties are not supported in the
/// shortestPath() function").
fn reject_shortest_path_properties(path: &PatternPath) -> Result<(), EngineError> {
    if !path.is_shortest_path_function {
        return Ok(());
    }
    let has_properties = path.elements.iter().any(|element| match element {
        PatternElement::Relationship(rel) => rel.has_properties,
        PatternElement::Node(_) | PatternElement::Quantified(_) => false,
    });
    if has_properties {
        return Err(EngineError::semantic(
            ErrorKind::InvalidPatternCharacter,
            "unsupported use of properties: properties are not supported in the shortestPath() function"
                .to_string(),
        ));
    }
    Ok(())
}

/// 42N34: a quantified path pattern may not also bind a path variable
/// (`MATCH ANY SHORTEST (p = (:A)--(:B))+`).
///
/// Scoped to a path pattern with an explicit selector: the CIP-28/CIP-40
/// stretch corpus — the spec harness's `cip-eval` suite (ADR-0520) —
/// deliberately exercises the plain named-QPP form
/// (`MATCH (p = (:A)-[:S]->(:B))+`, documented there as a GPM-only
/// feature current Neo4j does not implement), and those scenarios must
/// keep passing. Neo4j rejects the no-selector form too; this is a
/// stretch-floor deviation the harness reports (the retired in-repo
/// `cip-eval` baseline used to pin it).
fn reject_path_bound_in_qpp(path: &PatternPath) -> Result<(), EngineError> {
    if path.selector.is_none() {
        return Ok(());
    }
    let bound = path.elements.iter().find_map(|element| match element {
        PatternElement::Quantified(qpp) => qpp.subpath_variable.as_ref(),
        PatternElement::Node(_) | PatternElement::Relationship(_) => None,
    });
    match bound {
        Some(name) => Err(EngineError::semantic(
            ErrorKind::SyntaxError,
            format!(
                "path bound in quantified path pattern: path cannot be bound in a quantified path pattern (`{name}`)"
            ),
        )),
        None => Ok(()),
    }
}

/// 42I21: a parenthesized path pattern's trailing `WHERE` is evaluated
/// inside the group, so it may not reference the path variable the enclosing
/// pattern binds (`MATCH p = ((:A)-->+(:B) WHERE length(p) …)`).
fn reject_own_path_variable_in_group_where(path: &PatternPath) -> Result<(), EngineError> {
    let Some(where_expr) = &path.where_expr else {
        return Ok(());
    };
    let mut referenced = IdentHashSet::default();
    collect_expr_variables(where_expr, &mut referenced);
    for name in path.path_names() {
        if !referenced.contains(name) {
            continue;
        }
        // The group's *own* sub-path name is bound inside the group, so
        // reading it there is legal (`ANY SHORTEST (q = (…) WHERE length(q) …)`).
        // Only the enclosing path variable, declared outside the group, is
        // out of scope inside it (42I21).
        if path.subpath_variable.as_ref() == Some(name) {
            continue;
        }
        return Err(
            name.with_str(|text| out_of_scope_group_reference(text, ErrorKind::SyntaxError))
        );
    }
    Ok(())
}

/// The one 42I21 rejection text. The two sites keep distinct kinds on
/// purpose: a reference to the *enclosing* path variable is a plain
/// `SyntaxError` (the general 42I21 shape), while a reference to a sibling
/// path pattern's variable is the selective-path-pattern dependency rule
/// (`InvalidSelectivePathPatternDependency`), whose own TCK-aligned kind
/// predates this battery.
fn out_of_scope_group_reference(name: &str, kind: ErrorKind) -> EngineError {
    EngineError::semantic(
        kind,
        format!(
            "invalid reference to variable out of scope: not allowed to reference `{name}` from within a parenthesized/quantified path pattern in the same graph pattern"
        ),
    )
}

/// The REPEATABLE ELEMENTS redeclaration battery, over one `declared`
/// index and one "fewer than two paths" guard:
///
/// - **42I21** — a selective path pattern's group predicate may not
///   reference a variable declared by another path pattern of the same
///   graph pattern;
/// - **42N59** — a strict interior variable of a selective path pattern may
///   not be redeclared by another path pattern of the same graph pattern.
fn reject_repeatable_elements_violations(mc: &MatchClause) -> Result<(), EngineError> {
    if mc.pattern.len() < 2 {
        return Ok(());
    }
    let declared: Vec<IdentHashSet<SmallName>> = mc.pattern.iter().map(declared_names).collect();
    for (index, path) in mc.pattern.iter().enumerate() {
        if !selector_is_selective(path.selector.as_ref()) {
            continue;
        }
        // 42I21 first: the predicate's reads are the narrower question.
        let mut referenced = IdentHashSet::default();
        group_where_refs(path, &mut referenced);
        if let Some(name) = sibling_hit(&declared, index, &referenced) {
            return Err(name.with_str(|name| {
                out_of_scope_group_reference(name, ErrorKind::InvalidSelectivePathPatternDependency)
            }));
        }
        // 42N59: a strict interior variable redeclared by a sibling.
        let interior = interior_names(path);
        if let Some(name) = sibling_hit(&declared, index, &interior) {
            return Err(EngineError::semantic(
                ErrorKind::VariableAlreadyDefined,
                format!("variable `{name}` already declared"),
            ));
        }
    }
    Ok(())
}

/// The first name (in sorted order, so the rejection is deterministic) that
/// path `index`'s `names` shares with any *other* path's declarations.
fn sibling_hit<'a>(
    declared: &'a [IdentHashSet<SmallName>],
    index: usize,
    names: &'a IdentHashSet<SmallName>,
) -> Option<&'a SmallName> {
    declared
        .iter()
        .enumerate()
        .filter(|(other_index, _)| *other_index != index)
        .filter_map(|(_, other)| names.intersection(other).min())
        .min()
}

/// Every variable name a path pattern declares: its element variables
/// (including a quantified group's operand variables and sub-path name) plus
/// its own path names.
fn declared_names(path: &PatternPath) -> IdentHashSet<SmallName> {
    let mut names = IdentHashSet::default();
    for element in &path.elements {
        crate::ast::collect::collect_pattern_element_vars(element, &mut names);
    }
    names.extend(path.path_names().cloned());
    names
}

/// The strict interior variables of a path pattern: every node/relationship
/// variable that is not bound by the path's first or last node pattern. A
/// relationship variable is always interior; a quantified group's operand
/// variables are collected too (its own sub-path name is 42N34).
fn interior_names(path: &PatternPath) -> IdentHashSet<SmallName> {
    let mut names = IdentHashSet::default();
    let first_node = path
        .elements
        .iter()
        .position(|element| matches!(element, PatternElement::Node(_)));
    let last_node = path
        .elements
        .iter()
        .rposition(|element| matches!(element, PatternElement::Node(_)));
    for (index, element) in path.elements.iter().enumerate() {
        match element {
            PatternElement::Node(node) => {
                if Some(index) == first_node || Some(index) == last_node {
                    continue;
                }
                if let Some(name) = &node.variable {
                    names.insert(name.clone());
                }
            }
            PatternElement::Relationship(rel) => {
                if let Some(name) = &rel.variable {
                    names.insert(name.clone());
                }
            }
            PatternElement::Quantified(qpp) => {
                for inner in &qpp.elements {
                    crate::ast::collect::collect_pattern_element_vars(inner, &mut names);
                }
            }
        }
    }
    names
}

/// The variables referenced by a path pattern's group predicates: the
/// pattern's own trailing `WHERE` plus every quantified group's `WHERE`.
fn group_where_refs(path: &PatternPath, out: &mut IdentHashSet<SmallName>) {
    if let Some(where_expr) = &path.where_expr {
        collect_expr_variables(where_expr, out);
    }
    for element in &path.elements {
        if let PatternElement::Quantified(qpp) = element {
            if let Some(where_expr) = &qpp.where_expr {
                collect_expr_variables(where_expr, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(query: &str) -> Result<(), String> {
        crate::validate::parse_and_validate(query, validate_match_shape)
    }

    /// 42I04: `shortestPath()`/`allShortestPaths()` and explicit path modes
    /// are MATCH-only (live Neo4j 2026.09).
    #[test]
    fn shortest_path_and_path_modes_are_rejected_in_create_and_merge() {
        for query in [
            "MATCH (a), (b) MERGE shortestPath((a)-[:T]->(b))",
            "MATCH (a), (b) MERGE allShortestPaths((a)-[:T]->(b))",
            "MATCH (a), (b) CREATE shortestPath((a)-[:T]->(b))",
            "MATCH (a), (b) MERGE TRAIL (a)-[:T]->(b)",
            "MATCH (a), (b) CREATE ACYCLIC (a)-[:T]->(b)",
        ] {
            let err = validate(query).unwrap_err();
            assert!(err.contains("cannot be used in a"), "{query}: {err}");
        }
        // The same shapes stay legal in MATCH.
        assert!(validate("MATCH p = shortestPath((a)-[:T]->(b)) RETURN p").is_ok());
        assert!(validate("MATCH TRAIL (a)-[:T]->(b) RETURN a").is_ok());
    }

    /// 42N56: relationship properties are not supported inside
    /// `shortestPath()`/`allShortestPaths()`.
    #[test]
    fn shortest_path_relationship_properties_are_rejected() {
        for query in [
            "MATCH (a), (b), shortestPath((a)-[r* {x: 1}]->(b)) RETURN *",
            "MATCH (a), (b), allShortestPaths((a)-[*1..3 {x: 1}]->(b)) RETURN *",
            // An empty map is a written specification too (live-verified:
            // Neo4j answers 42N56 for `-[{}]-` exactly as for `-[{x: 1}]-`).
            "MATCH (a), (b), shortestPath((a)-[{}]->(b)) RETURN *",
        ] {
            let err = validate(query).unwrap_err();
            assert!(err.contains("properties"), "{query}: {err}");
        }
        assert!(validate("MATCH (a), (b), shortestPath((a)-[*]->(b)) RETURN *").is_ok());
    }
}
