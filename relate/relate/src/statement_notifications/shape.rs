//! Clause-shape notification detectors (ADR-0363 decisions 3/5): the
//! deprecated forms whose facts live in the statement's clause tree rather
//! than in a catalog lookup or a plan operator.
//!
//! The projection-subclause deprecations (ambiguous and complex references
//! in `ORDER BY`/`WHERE`), the `CREATE` self-reference, the repeated
//! relationship variable, the unsatisfiable relationship type expression,
//! the subquery and `SEARCH` shadowing facts, and the exhaustive
//! shortest-path predicate are all raised here. [`super::Facts`] owns the
//! accumulator and calls into this module; each detector appends to the
//! notification list it is handed.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use relate_ir::{
    expr_vars, is_agg_call, is_agg_expr, walk_pattern_payloads, CallScope, Clause, Expr, LabelExpr,
    OrderItem, PathPattern, PatternPayload, Projection, Query, SearchSpec,
};
use relate_model::wire::{Notification, NotificationKind};

use super::collect_label_names;
use crate::plan_node::patterns_share_an_anchor;

/// The statement-shape checks' shared work budget, in pair comparisons.
///
/// Two detectors cost the product of two attacker-widenable dimensions: the
/// type-expression satisfiability check tries every named type against the
/// whole expression (names × expression nodes), and the complex-reference
/// check tests every visited expression node against every projection item
/// (nodes × items). Both products are quadratic in a statement's width, and
/// the parser caps nesting depth, not width; a crafted `EXPLAIN` statement
/// within the transport's statement cap would otherwise spin for hours while
/// the per-query cancellation token goes unobserved by the tight loop.
///
/// Past this budget a detector declines the check and emits nothing. A
/// missed deprecation hint is the conservative direction — the notification
/// is advisory — and the decline is silent by construction: the detectors
/// return before appending. The bound sits above realistic statements (a
/// 2000-item projection ordered by a 2000-element list is ~8M) and well
/// below the pathological widths the statement cap admits (~10^12).
const SHAPE_CHECK_WORK_BUDGET: u64 = 16 * 1024 * 1024;

/// Whether a detector may spend `units` pair comparisons, or must decline
/// the check silently. One policy for every shape detector, so the bound
/// cannot drift between them.
fn within_work_budget(units: u64) -> bool {
    units <= SHAPE_CHECK_WORK_BUDGET
}

/// The `MATCH`-shape notifications: a nested multi-pattern cartesian (the
/// top-level form is the operator-scoped warning), repeated relationship
/// variables, unsatisfiable relationship type expressions, and the
/// exhaustive-shortest-path predicate.
pub(super) fn observe_match(
    notifications: &mut Vec<Notification>,
    patterns: &[PathPattern],
    where_clause: Option<&Expr>,
    nested: bool,
) {
    if nested && patterns.len() > 1 && !patterns_share_an_anchor(patterns) {
        notifications.push(Notification::new(
            NotificationKind::CartesianProduct,
            "The MATCH patterns share no join anchor.",
        ));
    }
    let mut repeated: BTreeMap<String, usize> = BTreeMap::new();
    for pattern in patterns {
        walk_pattern_payloads(pattern, &mut |payload| {
            let PatternPayload::Relationship(rel) = payload else {
                return;
            };
            if let Some(variable) = &rel.variable {
                // Text render: the repeated-rel counter is the String
                // notification domain.
                *repeated.entry(variable.to_string()).or_default() += 1;
            }
            if let Some(expression) = &rel.type_expr {
                if type_expression_unsatisfiable(expression) {
                    notifications.push(Notification::new(
                        NotificationKind::UnsatisfiableRelationshipTypeExpression,
                        "The relationship type expression cannot be satisfied because \
                         relationships have exactly one type.",
                    ));
                }
            }
        });
    }
    for (variable, count) in repeated {
        if count > 1 {
            notifications.push(Notification::new(
                NotificationKind::RepeatedRelationshipVariable,
                format!(
                    "The relationship variable `{variable}` is repeated in the same \
                     MATCH, which leads to no results."
                ),
            ));
        }
    }
    if let Some(where_clause) = where_clause {
        // One walk of the predicate, then a constant-time check per
        // pattern: the detector used to re-walk the whole `WHERE` for
        // every shortest-path pattern in the MATCH.
        let forcing = ExhaustivePredicateVars::collect(where_clause);
        if patterns.iter().any(|pattern| forcing.matches(pattern)) {
            notifications.push(Notification::new(
                NotificationKind::ExhaustiveShortestPath,
                "The query runs with exhaustive shortest path due to an existential \
                 predicate; separate the predicate from the MATCH with WITH where possible.",
            ));
        }
    }
}

/// The variables a `WHERE` clause names in the two positions that force an
/// exhaustive shortest-path evaluation: the list of a quantifier over the
/// relationship variable, and the first argument of a `length(…)` call over
/// the path variable. Collected in one walk so each pattern's check is a
/// pair of set lookups rather than a re-walk of the predicate. The names are
/// owned because `Expr::visit` hands the callback borrows that cannot escape
/// it; only the two forcing positions are copied.
#[derive(Default)]
struct ExhaustivePredicateVars {
    quantifier_lists: HashSet<String>,
    length_arguments: HashSet<String>,
}

impl ExhaustivePredicateVars {
    fn collect(expr: &Expr) -> Self {
        let mut facts = Self::default();
        expr.visit(&mut |node| match node {
            Expr::Quantifier { list, .. } => {
                if let Expr::Variable(variable) = list.as_ref() {
                    // Text render: the fact sets are the String sink the
                    // pattern probes read.
                    facts.quantifier_lists.insert(variable.to_string());
                }
            }
            Expr::Function { name, args, .. }
                if name.with_str(|n| n.eq_ignore_ascii_case("length")) =>
            {
                if let Some(Expr::Variable(variable)) = args.first() {
                    facts.length_arguments.insert(variable.to_string());
                }
            }
            _ => {}
        });
        facts
    }

    /// Whether this pattern's predicates force the exhaustive evaluation: a
    /// quantifier over its relationship variable, or a `length(…)` over one
    /// of its path names. Only a `shortestPath` pattern can be forced.
    fn matches(&self, pattern: &PathPattern) -> bool {
        if !pattern.is_shortest_path_function {
            return false;
        }
        pattern
            .rels
            .iter()
            .filter_map(|rel| rel.variable.as_ref())
            .any(|rel| rel.with_str(|text| self.quantifier_lists.contains(text)))
            || pattern
                .path_names()
                .any(|path| path.with_str(|text| self.length_arguments.contains(text)))
    }
}

/// The `CREATE` self-reference (Create.feature): a property definition of
/// any entity created in this clause reading another (or the same) entity
/// created in the same clause. One notification per (referencing property
/// expression, referenced entity) pair.
pub(super) fn observe_create(notifications: &mut Vec<Notification>, patterns: &[PathPattern]) {
    let created: BTreeSet<String> = patterns
        .iter()
        .flat_map(|pattern| {
            // Text render: the created-name set is the String sink the
            // property expressions' variable reads probe.
            pattern
                .nodes
                .iter()
                .filter_map(|node| node.variable.as_ref().map(|v| v.to_string()))
                .chain(
                    pattern
                        .rels
                        .iter()
                        .filter_map(|rel| rel.variable.as_ref().map(|v| v.to_string())),
                )
        })
        .collect();
    if created.is_empty() {
        return;
    }
    // The (property value, created name) pairs the scan below tests: declined
    // past the shared budget, like the sibling detectors, so a crafted `CREATE`
    // with a wide property map and many created entities cannot pin a core
    // before execution.
    let values: u64 = patterns
        .iter()
        .flat_map(|pattern| {
            pattern
                .nodes
                .iter()
                .map(|node| &node.properties)
                .chain(pattern.rels.iter().map(|rel| &rel.properties))
        })
        .map(|properties| match properties {
            Some(Expr::Map(items)) => items.len() as u64,
            _ => 0,
        })
        .sum();
    if !within_work_budget(values.saturating_mul(created.len() as u64)) {
        return;
    }
    for pattern in patterns {
        let payloads = pattern
            .nodes
            .iter()
            .map(|node| (&node.variable, &node.properties))
            .chain(
                pattern
                    .rels
                    .iter()
                    .map(|rel| (&rel.variable, &rel.properties)),
            );
        for (_, properties) in payloads {
            let Some(Expr::Map(items)) = properties else {
                continue;
            };
            for (_, value) in items {
                let mut vars = HashSet::new();
                expr_vars(value, &mut vars);
                for name in &created {
                    if vars.contains(name) {
                        notifications.push(Notification::new(
                            NotificationKind::CreateSelfReference,
                            format!(
                                "Creating an entity (`{name}`) and referencing that entity \
                                 in a property definition in the same CREATE is deprecated."
                            ),
                        ));
                    }
                }
            }
        }
    }
}

/// The projection-subclause deprecations (ProjectionSubclauseAcceptance,
/// GroupByAcceptance):
///
/// - an `ORDER BY`/`WHERE` expression referencing a variable that a
///   projection alias re-binds is an *ambiguous* reference (01N01);
/// - an `ORDER BY`/`WHERE` expression containing a complex projection item
///   expression as a proper sub-expression is a *complex* reference (01N01).
///
/// A bare variable naming a projection alias is the alias itself, not an
/// ambiguous reference; an aggregation is a recognized sort key, not a
/// complex projection item. The complex-reference deprecation only applies
/// under implicit grouping or `DISTINCT`, where the subclause cannot see
/// the pre-projection scope.
pub(super) fn observe_projection(
    notifications: &mut Vec<Notification>,
    items: &[Projection],
    order: &[OrderItem],
    where_clause: Option<&Expr>,
    distinct: bool,
) {
    if items.is_empty() {
        return;
    }
    let grouped = distinct || items.iter().any(|item| is_agg_expr(&item.expr));
    let mut item_vars = HashSet::new();
    for item in items {
        expr_vars(&item.expr, &mut item_vars);
    }
    // The alias sets render once (the String sink the expr_vars probes
    // read); a bare alias reference compares by text.
    let aliases: HashSet<String> = items
        .iter()
        .filter_map(|item| item.alias.as_ref().map(|a| a.to_string()))
        .collect();
    let shadowing: BTreeSet<String> = items
        .iter()
        .filter_map(|item| {
            let alias = item.alias.as_ref()?;
            let alias_text = alias.to_string();
            if matches!(&item.expr, Expr::Variable(variable) if variable == alias) {
                return None;
            }
            item_vars
                .contains(alias_text.as_str())
                .then_some(alias_text)
        })
        .collect();
    // The complex projection items, selected once. The per-pair loop below
    // used to re-run the shape test for every (subclause expression, item)
    // pair, so a large projection paid for the same answer once per pair.
    let complex: Vec<(usize, &Projection)> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| grouped && is_complex_projection_item(&item.expr))
        .collect();
    // The (subclause expression, shadowing name) pairs the ambiguous-reference
    // scan tests: declined past the shared budget, while the complex-reference
    // pass below keeps its own guard. One wide dimension therefore never
    // silences the other family.
    let shadowing_scan = within_work_budget(
        (order.len() as u64 + u64::from(where_clause.is_some()))
            .saturating_mul(shadowing.len() as u64),
    );
    for expr in order.iter().map(|item| &item.expr).chain(where_clause) {
        let bare_alias = matches!(expr, Expr::Variable(variable)
                if variable.with_str(|text| aliases.contains(text)));
        let mut vars = HashSet::new();
        expr_vars(expr, &mut vars);
        if !bare_alias && shadowing_scan {
            for name in &shadowing {
                if vars.contains(name.as_str()) {
                    notifications.push(Notification::new(
                        NotificationKind::AmbiguousSubclauseReference,
                        format!(
                            "The subclause expression contains an ambiguous reference to \
                             variable `{name}`; reference the projection item by an alias \
                             or use the GROUP BY clause."
                        ),
                    ));
                }
            }
        }
        if complex.is_empty() {
            continue;
        }
        for (index, item) in complex_references(expr, &complex) {
            let text = item
                .source_text
                .clone()
                .unwrap_or_else(|| item.output_name(index));
            notifications.push(Notification::new(
                NotificationKind::ComplexSubclauseReference,
                format!(
                    "The subclause expression contains a reference to the complex \
                     projection item expression `{text}`; reference it by an alias."
                ),
            ));
        }
    }
}

/// The variable-scope facts that need the clause tree: a `CALL { … }` body
/// variable shadowing an outer name (03N60), and a `SEARCH` index identifier
/// sharing a name with a variable in scope (03N63).
///
/// One structural recursion over the tree carries the scope it is in: each
/// clause list (a statement part, a `CALL { … }` arm, a `WHEN`/`ELSE`
/// branch, a `FOREACH` body) starts from the frame its enclosing position
/// supplies and grows one clause at a time by [`introduced_names`], so a
/// subquery body is analysed against the names actually visible at its CALL
/// clause. Imported names — a scope clause's or a scope-less body's legacy
/// leading `WITH` — are not shadowing.
///
/// Clauses nested in expression subqueries (`EXISTS`/`COUNT`/`COLLECT`) are
/// not part of this recursion: those queries live in expressions, not in the
/// clause tree.
pub(super) fn observe_subquery_scopes(notifications: &mut Vec<Notification>, parts: &[&[Clause]]) {
    // The per-subquery work scales with the statement-wide name set (a
    // wildcard import clones it): declined past the shared budget, like the
    // sibling detectors. The SEARCH family stays checked — its cost is per
    // index-specification name, not per scope.
    let mut declared: BTreeSet<String> = BTreeSet::new();
    let mut subqueries = 0u64;
    for part in parts {
        relate_ir::walk_clauses(
            part,
            &mut |clause| {
                if matches!(clause, Clause::CallSubquery { .. }) {
                    subqueries += 1;
                }
                declared.extend(introduced_names(clause));
            },
            &mut |_| {},
        );
    }
    let subquery_scan = within_work_budget(subqueries.saturating_mul(declared.len() as u64));
    // A UNION arm and a `NEXT` stage are independent statements: each is its
    // own clause list, so neither sees the other's names.
    for part in parts {
        observe_scope_frame(notifications, part, &Scope::new(), subquery_scan);
    }
}

/// The names in force at one point of the clause tree.
type Scope = BTreeSet<String>;

/// One structural recursion over a clause list, carrying the frame the list
/// runs in.
///
/// `outer` is the frame the enclosing position supplies (empty for a
/// top-level part, the frame at the `CALL { … }` clause for its body, plus
/// the loop variable for a `FOREACH` body). A clause sees the frame its
/// predecessors built; its own names join it for the clauses after it.
fn observe_scope_frame(
    notifications: &mut Vec<Notification>,
    clauses: &[Clause],
    outer: &Scope,
    subquery_scan: bool,
) {
    let mut scope = outer.clone();
    for clause in clauses {
        let own = introduced_names(clause);
        observe_scope_clause(notifications, clause, &scope, &own, subquery_scan);
        scope.extend(own);
    }
}

/// The per-clause half of [`observe_scope_frame`]: the `SEARCH` shadow test
/// against the frame before the clause (plus the clause's own pattern
/// variables, which its index specification sees), and the descent into
/// whatever clause lists the clause carries.
fn observe_scope_clause(
    notifications: &mut Vec<Notification>,
    clause: &Clause,
    scope: &Scope,
    own: &BTreeSet<String>,
    subquery_scan: bool,
) {
    if let Clause::Match {
        search: Some(search),
        ..
    } = clause
    {
        for name in search_index_names(&search.spec) {
            if scope.contains(name) || own.contains(name) {
                notifications.push(Notification::new(
                    NotificationKind::SearchIdentifierShadowingVariable,
                    format!(
                        "The identifier `{name}` in the index specification clause has \
                         the same name as a variable in scope."
                    ),
                ));
            }
        }
    }
    match clause {
        Clause::CallSubquery {
            query,
            scope: call_scope,
            ..
        } => {
            if subquery_scan {
                observe_call_body(notifications, query, call_scope, scope);
            }
            // The body runs in the frame at the CALL clause, whichever arm.
            observe_query_frames(notifications, query, scope, subquery_scan);
        }
        Clause::Conditional {
            branches,
            else_query,
        } => {
            for (_, query) in branches {
                observe_query_frames(notifications, query, scope, subquery_scan);
            }
            if let Some(query) = else_query.as_deref() {
                observe_query_frames(notifications, query, scope, subquery_scan);
            }
        }
        Clause::Foreach {
            variable, clauses, ..
        } => {
            let mut inner = scope.clone();
            inner.insert(variable.to_string());
            observe_scope_frame(notifications, clauses, &inner, subquery_scan);
        }
        _ => {}
    }
}

/// Every part of a nested [`Query`], each against the same `outer` frame:
/// UNION arms and `NEXT` stages do not see each other's names.
fn observe_query_frames(
    notifications: &mut Vec<Notification>,
    query: &Query,
    outer: &Scope,
    subquery_scan: bool,
) {
    for part in &query.parts {
        observe_scope_frame(notifications, &part.clauses, outer, subquery_scan);
    }
    if let Some(next) = query.next.as_deref() {
        observe_query_frames(notifications, next, outer, subquery_scan);
    }
}

/// The 03N60 test for one `CALL { … }` body: per UNION arm, the names the
/// arm introduces and does not import, kept to those the outer frame holds.
///
/// Each arm introduces its own names and imports its own, so the test runs
/// per arm: a name arm A declares is a shadow even when arm B imports a name
/// with the same spelling. One notification per spelling, even when several
/// arms shadow it.
fn observe_call_body(
    notifications: &mut Vec<Notification>,
    query: &Query,
    call_scope: &CallScope,
    outer: &Scope,
) {
    let own_by_arm: Vec<BTreeSet<String>> = query
        .parts
        .iter()
        .map(|part| part.clauses.iter().flat_map(introduced_names).collect())
        .collect();
    let imports_by_arm = call_imports(call_scope, query, outer);
    let shadowing: BTreeSet<&String> = own_by_arm
        .iter()
        .zip(imports_by_arm.iter())
        .flat_map(|(own, imports)| own.difference(imports))
        .filter(|name| outer.contains(name.as_str()))
        .collect();
    for name in shadowing {
        notifications.push(Notification::new(
            NotificationKind::SubqueryVariableShadowing,
            format!(
                "The variable `{name}` in the subquery uses the same name as a \
                 variable in the outer scope; import the outer one with \
                 `CALL ({name}) {{ … }}` or rename the inner one."
            ),
        ));
    }
}

/// The names a clause introduces into the scope after it (the IR analog of
/// the front's `clause_introduced_names`): pattern variables for
/// MATCH/CREATE/MERGE, a WITH item's output name, and the alias of
/// UNWIND/LOAD CSV/FOREACH or a procedure CALL's YIELD. `*` is not a name.
fn introduced_names(clause: &Clause) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    match clause {
        Clause::Match {
            patterns, search, ..
        } => {
            for pattern in patterns {
                collect_pattern_names(pattern, &mut names);
            }
            if let Some(score) = search.as_ref().and_then(|search| search.score_var.as_ref()) {
                names.insert(score.to_string());
            }
        }
        Clause::Create { patterns } => {
            for pattern in patterns {
                collect_pattern_names(pattern, &mut names);
            }
        }
        Clause::Merge { pattern, .. } => collect_pattern_names(pattern, &mut names),
        Clause::Unwind { alias, .. } => {
            names.insert(alias.to_string());
        }
        Clause::LoadCsv { variable, .. } => {
            names.insert(variable.to_string());
        }
        Clause::Call { yield_items, .. } => {
            names.extend(yield_items.effective_names().map(|name| name.to_string()));
        }
        // A `RETURN` is terminal and exports columns rather than binding
        // names in a scope later clauses (or an outer scope) can shadow; a
        // `WITH` replaces the scope, so its outputs count.
        Clause::With { items, .. } => {
            for (index, item) in items.iter().enumerate() {
                if !matches!(item.expr, Expr::Star) {
                    names.insert(item.output_name(index));
                }
            }
        }
        Clause::Foreach { variable, .. } => {
            names.insert(variable.to_string());
        }
        _ => {}
    }
    names
}

/// Every variable a path pattern binds: its path names, and every node and
/// relationship variable including quantified groups.
fn collect_pattern_names(pattern: &PathPattern, out: &mut BTreeSet<String>) {
    // Text render: the collected scope names are the String sink.
    out.extend(pattern.path_names().map(|name| name.to_string()));
    walk_pattern_payloads(pattern, &mut |payload| match payload {
        PatternPayload::Node(node) => {
            if let Some(variable) = &node.variable {
                out.insert(variable.to_string());
            }
        }
        PatternPayload::Relationship(rel) => {
            if let Some(variable) = &rel.variable {
                out.insert(variable.to_string());
            }
        }
        PatternPayload::Quantified(_) => {}
    });
}

/// The names each UNION arm of a `CALL { … }` body imports from its outer
/// scope, in arm order.
///
/// A scope clause declares one import set for the whole body (`*` imports
/// everything in scope), so every arm reports it. A scope-less body imports
/// per arm from that arm's own legacy leading `WITH`
/// ([`relate_ir::SingleQuery::leading_imports`] — the same derivation
/// the planner's 42I28 rule consults): one arm's import must never suppress
/// a sibling arm's shadowing diagnostic.
fn call_imports(
    scope: &CallScope,
    query: &Query,
    outer: &BTreeSet<String>,
) -> Vec<BTreeSet<String>> {
    match scope {
        CallScope::Wildcard => query.parts.iter().map(|_| outer.clone()).collect(),
        CallScope::Vars(names) => {
            // Text render: the import sets are the String scope domain.
            let declared: BTreeSet<String> = names.iter().map(|name| name.to_string()).collect();
            query.parts.iter().map(|_| declared.clone()).collect()
        }
        CallScope::None => query
            .parts
            .iter()
            .map(|part| part.leading_imports(outer))
            .collect(),
    }
}

/// The index names a `SEARCH` source specification names.
fn search_index_names(spec: &SearchSpec) -> Vec<&str> {
    match spec {
        SearchSpec::Index(index) => vec![index.index_name.as_str()],
        SearchSpec::Combine(combine) => combine
            .sources
            .iter()
            .flat_map(|source| search_index_names(&source.spec))
            .collect(),
    }
}

/// Whether a projection item is a *complex* expression — one the ORDER
/// BY/WHERE deprecation asks to be aliased. A bare variable, a literal and
/// a parameter are simple; an aggregate is a recognized sort key.
fn is_complex_projection_item(expr: &Expr) -> bool {
    !is_agg_call(expr)
        && !matches!(
            expr,
            Expr::RuntimeValue(_)
                | Expr::Null
                | Expr::Bool(_)
                | Expr::Int(_)
                | Expr::Float(_)
                | Expr::String(_)
                | Expr::Parameter(_)
                | Expr::Variable(_)
                | Expr::Star
                | Expr::CountStar
        )
}

/// The complex projection items `expr` contains as a *proper* sub-expression,
/// in projection order.
///
/// One pre-order walk of the expression's children: every visited node is
/// tested against the still-unmatched candidates, and a match drops its
/// candidate so the walk never re-tests a pair. Nothing is materialized, so
/// the walk answers every candidate per expression rather than once per
/// (expression, item) pair.
///
/// The walk's cost is nodes × items, so it declines once that product passes
/// [`SHAPE_CHECK_WORK_BUDGET`]: a wide projection ordered by a wide
/// expression would otherwise pin a core, and a partial answer (some
/// references reported) would be no more truthful than none.
///
/// The root is skipped by walking `expr.children()`: an expression equal to
/// the whole subclause expression is the projection item itself, a
/// recognized sort key rather than a reference. Identity would be a cheaper
/// key than the structural comparison, but it is not available here: `visit`
/// hands the callback borrows that cannot escape the closure, and lowering
/// builds `ORDER BY` expressions from a separately parsed syntax tree, so
/// the same expression reaches this check as a structural copy rather than a
/// shared node.
fn complex_references<'a>(
    expr: &Expr,
    candidates: &[(usize, &'a Projection)],
) -> Vec<(usize, &'a Projection)> {
    if candidates.is_empty() {
        return Vec::new();
    }
    // Count the visited nodes first — a comparison-free walk — so a
    // pathological nodes × items pair declines before any of the pair work
    // rather than partway through it.
    let mut nodes = 0u64;
    for child in expr.children() {
        child.visit(&mut |_| nodes += 1);
    }
    if !within_work_budget(nodes.saturating_mul(candidates.len() as u64)) {
        return Vec::new();
    }
    let mut matched = vec![false; candidates.len()];
    for child in expr.children() {
        child.visit(&mut |node| {
            for (position, (_, item)) in candidates.iter().enumerate() {
                if !matched[position] && node == &item.expr {
                    matched[position] = true;
                }
            }
        });
    }
    candidates
        .iter()
        .zip(matched)
        .filter_map(|(candidate, matched)| matched.then_some(*candidate))
        .collect()
}

/// Whether a relationship type expression admits no single type: the
/// exactly-one-type universe is the expression's own names plus one fresh
/// name no literal mentions. A dynamic part makes the answer undecidable
/// here, so it declines (returns `false`).
///
/// The check tries every candidate type against the whole expression, so its
/// cost is candidates × expression nodes. Every candidate can fail — that is
/// what an unsatisfiable expression is — so nothing short-circuits. Past
/// [`SHAPE_CHECK_WORK_BUDGET`] the check declines (returns `false`, emitting
/// no notification): a wide crafted conjunction must not pin a core.
fn type_expression_unsatisfiable(expr: &LabelExpr) -> bool {
    if label_expression_has_dynamic(expr) {
        return false;
    }
    let mut names = BTreeSet::new();
    collect_label_names(expr, &mut names);
    let candidates = names.len() as u64 + 1;
    if !within_work_budget(candidates.saturating_mul(label_expression_size(expr) as u64)) {
        return false;
    }
    let mut candidates: Vec<Option<&str>> = names.iter().map(|name| Some(name.as_str())).collect();
    candidates.push(None);
    !candidates
        .into_iter()
        .any(|candidate| label_expression_satisfied(expr, candidate))
}

/// The expression's node count — the per-candidate cost of
/// [`label_expression_satisfied`].
fn label_expression_size(expr: &LabelExpr) -> usize {
    match expr {
        LabelExpr::Label(_) | LabelExpr::Wildcard | LabelExpr::Dynamic { .. } => 1,
        LabelExpr::Not(inner) => 1 + label_expression_size(inner),
        LabelExpr::And(items) | LabelExpr::Or(items) => {
            1 + items.iter().map(label_expression_size).sum::<usize>()
        }
    }
}

fn label_expression_has_dynamic(expr: &LabelExpr) -> bool {
    match expr {
        LabelExpr::Dynamic { .. } => true,
        LabelExpr::Not(inner) => label_expression_has_dynamic(inner),
        LabelExpr::And(items) | LabelExpr::Or(items) => {
            items.iter().any(label_expression_has_dynamic)
        }
        LabelExpr::Label(_) | LabelExpr::Wildcard => false,
    }
}

/// The expression's truth under a relationship whose single type is
/// `candidate`; `None` is a type the expression does not name.
fn label_expression_satisfied(expr: &LabelExpr, candidate: Option<&str>) -> bool {
    match expr {
        LabelExpr::Label(name) => name.with_str(|text| candidate == Some(text)),
        LabelExpr::Wildcard => true,
        LabelExpr::Not(inner) => !label_expression_satisfied(inner, candidate),
        LabelExpr::And(items) => items
            .iter()
            .all(|item| label_expression_satisfied(item, candidate)),
        LabelExpr::Or(items) => items
            .iter()
            .any(|item| label_expression_satisfied(item, candidate)),
        LabelExpr::Dynamic { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relate_names::SmallName;

    fn label(name: &str) -> LabelExpr {
        LabelExpr::Label(name.into())
    }

    /// ADR-0363: a relationship has exactly one type, so a conjunction of
    /// two distinct type names — or a conjunction of a type with a
    /// disjunction of other types — admits no type. A disjunction and a
    /// negation stay satisfiable; a dynamic part declines the check.
    #[test]
    fn relationship_type_expression_satisfiability_follows_the_one_type_rule() {
        assert!(type_expression_unsatisfiable(&LabelExpr::And(vec![
            label("A"),
            label("B"),
        ])));
        assert!(type_expression_unsatisfiable(&LabelExpr::And(vec![
            LabelExpr::Not(Box::new(label("A"))),
            label("B"),
            LabelExpr::Or(vec![label("C"), label("D")]),
        ])));
        assert!(!type_expression_unsatisfiable(&LabelExpr::Or(vec![
            label("A"),
            label("B"),
        ])));
        assert!(!type_expression_unsatisfiable(&label("A")));
        assert!(!type_expression_unsatisfiable(&LabelExpr::Not(Box::new(
            label("A")
        ))));
        assert!(!type_expression_unsatisfiable(&LabelExpr::And(vec![
            label("A"),
            LabelExpr::Dynamic {
                expr: Box::new(Expr::Parameter("p".into())),
                quantifier: relate_ir::DynamicLabelQuantifier::Default,
            },
        ])));
    }

    /// A crafted wide conjunction must not pay names × expression nodes:
    /// every candidate fails (which is the input's point), so nothing
    /// short-circuits. Past the shared budget the check declines and returns
    /// `false`; a small conjunction is still checked normally.
    #[test]
    fn wide_type_expression_declines_past_the_work_budget() {
        // (WIDTH + 1) candidate types × (WIDTH + 1) expression nodes is well
        // past SHAPE_CHECK_WORK_BUDGET.
        const WIDTH: usize = 5000;
        let wide = LabelExpr::And(
            (0..WIDTH)
                .map(|index| label(&format!("T{index}")))
                .collect(),
        );
        assert!(
            !type_expression_unsatisfiable(&wide),
            "a wide conjunction declines instead of scanning every candidate"
        );
        assert!(
            type_expression_unsatisfiable(&LabelExpr::And(vec![label("A"), label("B")])),
            "a small conjunction is still checked"
        );
    }

    /// The sibling scans carry the same budget: a `CREATE` whose property map
    /// and created-entity count multiply past `SHAPE_CHECK_WORK_BUDGET`
    /// declines (no notification) instead of testing every pair.
    #[test]
    fn wide_create_self_reference_scan_declines_past_the_work_budget() {
        // WIDTH property values × WIDTH created entities is well past the
        // budget; the values reference the created names, so a checked scan
        // would emit WIDTH notifications.
        const WIDTH: usize = 5000;
        let mut body = String::from("(n0 {p: 1})");
        for index in 1..WIDTH {
            body.push_str(&format!(", (n{index} {{q: n0.p}})"));
        }
        // The self-reference notification is Cypher 5's 01N00; Cypher 25
        // rejects the same shape (42I58) at validation, so the notification
        // scan is exercised under an explicit `CYPHER 5` preamble.
        let query = relate_front::parse_and_lower(&format!("CYPHER 5 CREATE {body}")).unwrap();
        let Clause::Create { patterns } = &query.parts[0].clauses[0] else {
            panic!("expected a CREATE clause");
        };
        let mut notifications = Vec::new();
        observe_create(&mut notifications, patterns);
        assert!(
            notifications.is_empty(),
            "a wide CREATE declines instead of testing every (value, name) pair"
        );

        let small =
            relate_front::parse_and_lower("CYPHER 5 CREATE (a {p: 1}), (b {q: a.p})").unwrap();
        let Clause::Create { patterns } = &small.parts[0].clauses[0] else {
            panic!("expected a CREATE clause");
        };
        let mut notifications = Vec::new();
        observe_create(&mut notifications, patterns);
        assert_eq!(notifications.len(), 1, "a small CREATE is still checked");
    }

    /// A complex projection item is anything but a variable, literal, or
    /// parameter; aggregates are recognized sort keys and stay out.
    #[test]
    fn complex_projection_items_exclude_variables_literals_and_aggregates() {
        assert!(!is_complex_projection_item(&Expr::Variable("a".into())));
        assert!(!is_complex_projection_item(&Expr::Int(1)));
        assert!(!is_complex_projection_item(&Expr::Parameter("p".into())));
        assert!(!is_complex_projection_item(&Expr::Function {
            name: "sum".into(),
            distinct: false,
            args: vec![Expr::Variable("a".into())],
        }));
        assert!(is_complex_projection_item(&Expr::Function {
            name: "toInteger".into(),
            distinct: false,
            args: vec![Expr::Variable("a".into())],
        }));
        assert!(is_complex_projection_item(&Expr::Property {
            expr: Box::new(Expr::Variable("a".into())),
            key: "p".into(),
        }));
    }

    /// A `USE`-headed importing WITH still imports: the leading clause is
    /// the first non-`USE` one (`SingleQuery::leading_importing_with`), so
    /// the body rebinding `x` through it is an import, not a shadow (the
    /// previous first-clause-only derivation reported 03N60 here).
    /// The one observe-and-count scaffold the subquery-shadowing tests
    /// share: walk the statement, run the detector, count its 03N60 rows.
    fn subquery_shadowing_count(query: &relate_ir::Query) -> usize {
        let mut parts = Vec::new();
        relate_planner::walk_query_parts(query, &mut |part| parts.push(part));
        let mut notifications = Vec::new();
        observe_subquery_scopes(&mut notifications, &parts);
        notifications
            .iter()
            .filter(|notification| {
                notification.kind() == NotificationKind::SubqueryVariableShadowing
            })
            .count()
    }

    #[test]
    fn subquery_shadowing_sees_a_use_headed_importing_with() {
        let query = relate_front::parse_and_lower(
            "MATCH (x:Person) CALL { USE neo4j WITH x RETURN x AS y } RETURN x, y",
        )
        .unwrap();
        assert_eq!(
            subquery_shadowing_count(&query),
            0,
            "a USE-headed importing WITH imports `x`"
        );
    }

    /// WB2: imports are per UNION arm. Arm 2's importing WITH must not
    /// suppress arm 1's shadow of the same name.
    #[test]
    fn subquery_shadowing_is_per_union_arm() {
        // `x` is in scope outside; arm 1 rebinds it, arm 2 imports it. Arm
        // 1 still shadows (the flat statement-wide import set hid this).
        let per_arm = relate_front::parse_and_lower(
            "MATCH (x:Old) CALL { MATCH (x:Inner) RETURN x AS y \
             UNION WITH x RETURN x AS y } RETURN x, y",
        )
        .unwrap();
        assert_eq!(
            subquery_shadowing_count(&per_arm),
            1,
            "arm 1's shadow is not suppressed by arm 2's import"
        );
    }

    /// A name bound inside one `CALL { … }` body is local to it: a sibling
    /// body later in the statement walk does not see it, so rebinding the
    /// same spelling there shadows nothing. The flat pre-order occurrence
    /// test reported this as a shadow because the first body's `x` preceded
    /// the second body in the walk.
    #[test]
    fn subquery_shadowing_does_not_leak_across_sibling_bodies() {
        let siblings = relate_front::parse_and_lower(
            "CALL () { MATCH (x:First) RETURN x AS y } \
             CALL () { MATCH (x:Second) RETURN x AS z } RETURN y, z",
        )
        .unwrap();
        assert_eq!(
            subquery_shadowing_count(&siblings),
            0,
            "a sibling body's local name is not in the second body's outer scope"
        );
    }

    /// The complex-reference check sees every nested occurrence of a
    /// projection item's expression — matched structurally, because lowering
    /// builds the subclause expression independently of the projection list —
    /// but not the root: an expression equal to the whole subclause
    /// expression is the projection item itself, a recognized sort key.
    #[test]
    fn subquery_shadowing_ignores_names_introduced_after_the_body() {
        // `n` is bound inside the body and rebound by a *later* clause: it is
        // not in the subquery's outer scope, so nothing shadows. The
        // statement-wide occurrence test used to report it.
        let after = relate_front::parse_and_lower(
            "CALL () { MATCH (n:Old) RETURN count(n) AS c } MATCH (n:New) RETURN n.title, c",
        )
        .unwrap();
        assert_eq!(
            subquery_shadowing_count(&after),
            0,
            "a name introduced after the subquery shadows nothing"
        );

        // The control: a name bound *before* the body is in scope, so the
        // body rebinding it still shadows.
        let before = relate_front::parse_and_lower(
            "MATCH (n:Old) CALL () { MATCH (n:New) RETURN n AS x } RETURN n",
        )
        .unwrap();
        assert_eq!(
            subquery_shadowing_count(&before),
            1,
            "a name bound before the body is in its outer scope"
        );
    }

    #[test]
    fn complex_references_find_nested_occurrences_but_not_the_root() {
        let inner = Expr::Binary {
            op: relate_ir::BinaryOp::Add,
            left: Box::new(Expr::Variable("a".into())),
            right: Box::new(Expr::Int(0)),
        };
        let outer = Expr::Binary {
            op: relate_ir::BinaryOp::Add,
            left: Box::new(inner.clone()),
            right: Box::new(Expr::Int(1)),
        };
        let projection = |expr: Expr| Projection {
            expr,
            alias: None,
            source_text: None,
        };
        let nested = projection(inner.clone());
        let exact = projection(outer.clone());
        let elsewhere = projection(Expr::Variable("b".into()));
        let candidates = [(0, &nested), (1, &exact), (2, &elsewhere)];
        let found = complex_references(&outer, &candidates);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, 0, "the nested occurrence is a reference");
        assert!(
            complex_references(&inner, &candidates).is_empty(),
            "an equal root is the projection item itself, not a reference"
        );
    }

    /// The complex-reference detector walks the subclause expression once,
    /// not once per (expression, item) pair: a projection and an `ORDER BY`
    /// this large would be quadratic in the pairing and is not in the walk.
    #[test]
    fn complex_reference_detection_is_one_walk_per_subclause_expression() {
        const WIDTH: usize = 2000;
        let items: Vec<Projection> = (0..WIDTH)
            .map(|index| Projection {
                expr: Expr::Property {
                    expr: Box::new(Expr::Variable("n".into())),
                    key: SmallName::from_name(&format!("p{index}")),
                },
                alias: Some(SmallName::from_name(&format!("a{index}"))),
                source_text: None,
            })
            .collect();
        let order = OrderItem {
            expr: Expr::List(items.iter().map(|item| item.expr.clone()).collect()),
            ascending: true,
        };
        let mut notifications = Vec::new();
        observe_projection(&mut notifications, &items, &[order], None, true);
        assert_eq!(
            notifications
                .iter()
                .filter(|notification| {
                    notification.kind() == NotificationKind::ComplexSubclauseReference
                })
                .count(),
            WIDTH,
            "every projected expression is found in the subclause expression"
        );
    }

    /// A wide projection checked against a wide subclause expression declines
    /// at the shared node × item work budget instead of comparing every pair.
    #[test]
    fn complex_reference_check_declines_past_the_work_budget() {
        const WIDTH: usize = 3000;
        let items: Vec<Projection> = (0..WIDTH)
            .map(|index| Projection {
                expr: Expr::Property {
                    expr: Box::new(Expr::Variable("n".into())),
                    key: SmallName::from_name(&format!("p{index}")),
                },
                alias: Some(SmallName::from_name(&format!("a{index}"))),
                source_text: None,
            })
            .collect();
        let candidates: Vec<(usize, &Projection)> = items.iter().enumerate().collect();
        let wide = Expr::List(items.iter().map(|item| item.expr.clone()).collect());
        // 2 × WIDTH visited nodes × WIDTH items is past the budget.
        assert!(
            complex_references(&wide, &candidates).is_empty(),
            "a wide nodes × items pair declines instead of comparing every pair"
        );
    }

    /// The complex-reference deprecation only applies when the projection
    /// groups (`DISTINCT` or an aggregation); a plain non-grouping
    /// projection keeps its pre-projection scope visible to `ORDER BY`.
    #[test]
    fn complex_reference_requires_grouping_or_distinct() {
        let item = Projection {
            expr: Expr::Property {
                expr: Box::new(Expr::Variable("n".into())),
                key: "name".into(),
            },
            alias: Some("x".into()),
            source_text: None,
        };
        let order = OrderItem {
            expr: Expr::Binary {
                op: relate_ir::BinaryOp::Add,
                left: Box::new(item.expr.clone()),
                right: Box::new(Expr::Int(1)),
            },
            ascending: true,
        };
        let mut plain = Vec::new();
        observe_projection(
            &mut plain,
            std::slice::from_ref(&item),
            &[order.clone()],
            None,
            false,
        );
        assert!(plain.is_empty(), "a non-grouping projection does not warn");
        let mut grouped = Vec::new();
        observe_projection(&mut grouped, &[item], &[order], None, true);
        assert_eq!(grouped.len(), 1, "DISTINCT makes the reference deprecated");
    }

    /// A scope-less call's legacy leading `WITH` imports bare variables and
    /// the `*` star — one set per UNION arm.
    #[test]
    fn call_imports_read_each_arms_leading_with() {
        let query =
            relate_front::parse_and_lower("CALL { WITH *, a RETURN a AS x } RETURN x").unwrap();
        let Clause::CallSubquery {
            query: body, scope, ..
        } = &query.parts[0].clauses[0]
        else {
            panic!("expected a CALL subquery");
        };
        let outer = BTreeSet::from(["a".to_string(), "b".to_string()]);
        let imports = call_imports(scope, body, &outer);
        assert_eq!(imports.len(), body.parts.len());
        assert!(imports[0].contains("a"), "bare variable imported");
        assert!(imports[0].contains("b"), "star imports the outer scope");
    }

    /// A named subpath binds the same path value under its own name, so a
    /// `length(…)` over the subpath name forces the exhaustive evaluation
    /// just as one over the outer path variable does.
    #[test]
    fn named_subpath_length_forces_exhaustive_shortest_path() {
        let query = relate_front::parse_and_lower(
            "MATCH p = shortestPath((q = (a)-[r*]-(b))) WHERE length(q) > 1 RETURN p",
        )
        .unwrap();
        let Clause::Match {
            patterns,
            where_clause,
            ..
        } = &query.parts[0].clauses[0]
        else {
            panic!("expected a MATCH clause");
        };
        let mut notifications = Vec::new();
        observe_match(&mut notifications, patterns, where_clause.as_ref(), false);
        assert_eq!(
            notifications
                .iter()
                .filter(|notification| {
                    notification.kind() == NotificationKind::ExhaustiveShortestPath
                })
                .count(),
            1,
            "a length over the named subpath forces the exhaustive evaluation"
        );
    }

    /// A multi-relationship shortest path can carry its quantifier on any
    /// relationship, not just the first: a `[*]` on a later relationship
    /// variable forces the exhaustive evaluation too.
    #[test]
    fn later_relationship_quantifier_forces_exhaustive_shortest_path() {
        let query = relate_front::parse_and_lower(
            "MATCH p = shortestPath((a)-[r]->(b)<-[s*]-(c)) WHERE any(x IN s WHERE x IS NOT NULL) RETURN p",
        )
        .unwrap();
        let Clause::Match {
            patterns,
            where_clause,
            ..
        } = &query.parts[0].clauses[0]
        else {
            panic!("expected a MATCH clause");
        };
        let mut notifications = Vec::new();
        observe_match(&mut notifications, patterns, where_clause.as_ref(), false);
        assert_eq!(
            notifications
                .iter()
                .filter(|notification| {
                    notification.kind() == NotificationKind::ExhaustiveShortestPath
                })
                .count(),
            1,
            "a quantifier over a later relationship variable forces the exhaustive evaluation"
        );
    }
}
