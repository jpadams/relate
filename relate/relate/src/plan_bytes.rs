//! Retained logical size of a compiled statement and a plan-cache entry.
//!
//! The walk charges every clause and expression the IR walker visits, plus
//! the string payloads those nodes own. `Value::logical_bytes` covers
//! extracted literals. Sizes round up to the workspace quantum at the
//! reservation, not here.

use std::cell::Cell;

use relate_core::{str_logical_bytes, Value};
use relate_ir::{walk_clauses, walk_query, Clause, Expr, MapProjectionItem, Query};
use relate_names::SmallName;
use relate_planner::LogicalPlan;

/// Heap overhead charged for one visited clause or expression node.
const NODE_BYTES: u64 = 64;

/// Logical bytes of one cache entry: template, lowered query, plan, raw
/// text, and extracted values.
pub(crate) fn entry_bytes(
    template: &str,
    query: &Query,
    plan: &LogicalPlan,
    text: &str,
    values: &[(String, Value)],
) -> u64 {
    str_logical_bytes(template)
        .saturating_add(query_bytes(query))
        .saturating_add(plan_bytes(plan))
        .saturating_add(str_logical_bytes(text))
        .saturating_add(values_bytes(values))
}

/// Logical bytes of the plan an original compile keeps while it runs.
pub(crate) fn plan_bytes(plan: &LogicalPlan) -> u64 {
    let mut bytes = NODE_BYTES;
    let mut stage = Some(plan);
    while let Some(current) = stage {
        for part in &current.parts {
            add_clauses(part, &mut bytes);
        }
        stage = current.next.as_deref();
    }
    bytes
}

fn query_bytes(query: &Query) -> u64 {
    let bytes = Cell::new(NODE_BYTES);
    walk_query(
        query,
        &mut |clause| add(&bytes, NODE_BYTES.saturating_add(clause_payload(clause))),
        &mut |expr| add(&bytes, NODE_BYTES.saturating_add(expr_payload(expr))),
    );
    bytes.get()
}

fn add_clauses(clauses: &[Clause], total: &mut u64) {
    let bytes = Cell::new(0u64);
    walk_clauses(
        clauses,
        &mut |clause| add(&bytes, NODE_BYTES.saturating_add(clause_payload(clause))),
        &mut |expr| add(&bytes, NODE_BYTES.saturating_add(expr_payload(expr))),
    );
    *total = total.saturating_add(bytes.get());
}

fn add(bytes: &Cell<u64>, more: u64) {
    bytes.set(bytes.get().saturating_add(more));
}

fn values_bytes(values: &[(String, Value)]) -> u64 {
    values.iter().fold(0u64, |total, (name, value)| {
        total
            .saturating_add(str_logical_bytes(name))
            .saturating_add(value.logical_bytes())
    })
}

fn name_bytes(name: &SmallName) -> u64 {
    name.with_str(str_logical_bytes)
}

fn clause_payload(clause: &Clause) -> u64 {
    match clause {
        Clause::Call { name, .. } => name_bytes(name),
        Clause::Unwind { alias, .. }
        | Clause::Foreach {
            variable: alias, ..
        } => name_bytes(alias),
        Clause::With { items, .. } | Clause::Return { items, .. } => {
            items.iter().fold(0u64, |total, item| {
                total
                    .saturating_add(item.alias.as_ref().map(name_bytes).unwrap_or(0))
                    .saturating_add(
                        item.source_text
                            .as_deref()
                            .map(str_logical_bytes)
                            .unwrap_or(0),
                    )
            })
        }
        Clause::CreateIndex {
            name,
            label,
            property,
            ..
        } => name
            .as_deref()
            .map(str_logical_bytes)
            .unwrap_or(0)
            .saturating_add(name_bytes(label))
            .saturating_add(name_bytes(property)),
        Clause::DropIndex { target, .. } => match target {
            relate_ir::DropIndexTarget::Name(name) => str_logical_bytes(name),
            relate_ir::DropIndexTarget::Schema { label, property } => {
                name_bytes(label).saturating_add(name_bytes(property))
            }
        },
        Clause::LoadCsv {
            variable,
            field_terminator,
            ..
        } => name_bytes(variable).saturating_add(
            field_terminator
                .as_deref()
                .map(str_logical_bytes)
                .unwrap_or(0),
        ),
        Clause::Catalog(_)
        | Clause::Match { .. }
        | Clause::Create { .. }
        | Clause::Merge { .. }
        | Clause::Delete { .. }
        | Clause::Set { .. }
        | Clause::Remove { .. }
        | Clause::ShowIndexes
        | Clause::Finish
        | Clause::CallSubquery { .. }
        | Clause::Conditional { .. }
        | Clause::Use { .. } => 0,
    }
}

fn expr_payload(expr: &Expr) -> u64 {
    match expr {
        Expr::String(text) | Expr::Parameter(text) => str_logical_bytes(text),
        Expr::RuntimeValue(value) => value.logical_bytes(),
        Expr::Variable(name)
        | Expr::Property { key: name, .. }
        | Expr::Function { name, .. }
        | Expr::ListComp { ident: name, .. }
        | Expr::Quantifier { ident: name, .. } => name_bytes(name),
        Expr::MapComp {
            key_ident,
            value_ident,
            ..
        } => key_ident
            .as_ref()
            .map(name_bytes)
            .unwrap_or(0)
            .saturating_add(name_bytes(value_ident)),
        Expr::Reduce {
            accumulator, ident, ..
        }
        | Expr::AllReduce {
            accumulator, ident, ..
        } => name_bytes(accumulator).saturating_add(name_bytes(ident)),
        Expr::IsNormalized { form, .. } => form.as_deref().map(str_logical_bytes).unwrap_or(0),
        Expr::Map(entries) => entries.iter().fold(0u64, |total, (key, _)| {
            total.saturating_add(str_logical_bytes(key))
        }),
        Expr::MapProjection { items, .. } => items.iter().fold(0u64, |total, item| {
            let name = match item {
                MapProjectionItem::KeyValue { key, .. }
                | MapProjectionItem::Property { key }
                | MapProjectionItem::Variable { name: key } => Some(key),
                MapProjectionItem::AllProperties => None,
            };
            total.saturating_add(name.map(name_bytes).unwrap_or(0))
        }),
        Expr::Null
        | Expr::Bool(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Star
        | Expr::DynamicProperty { .. }
        | Expr::LabelPredicate { .. }
        | Expr::List(_)
        | Expr::Unary { .. }
        | Expr::Binary { .. }
        | Expr::CountStar
        | Expr::Case { .. }
        | Expr::Subscript { .. }
        | Expr::Slice { .. }
        | Expr::IsType { .. }
        | Expr::ChronoCompare { .. }
        | Expr::DistanceMetric(_)
        | Expr::PatternPred { .. }
        | Expr::PatternComp { .. }
        | Expr::ShortestPath { .. }
        | Expr::ExistsSubquery { .. }
        | Expr::CountSubquery { .. }
        | Expr::CollectSubquery { .. } => 0,
    }
}
