//! The non-trivial clause arms the dispatcher delegates to: CREATE, MERGE,
//! DELETE, SET, REMOVE, and UNWIND. The dispatcher keeps the loop, the
//! MATCH arm (which needs its local fold machinery), and the shared
//! clause-exit reporting.

use std::collections::BTreeMap;

use relate_core::{Result, Value};
use relate_ir::{Clause, Expr, MatchMode};
use relate_names::SmallName;

use crate::ctx::{ExecCtx, QueryCtx};
use crate::eval::{eval, into_items};
use crate::overlay::ExecGraph;
use crate::row::Row;

use super::create::{
    create_patterns, merge_match_pattern, patterns_have_graph_sensitive_expr,
    reject_null_merge_props, resolve_create_patterns,
};
use super::match_clause::match_paths;
use super::pushdown::Residual;
use super::scope::refresh_rows;
use super::unwind::Source;
use super::write::{apply_resolved_removes, apply_sets, delete_values, resolve_removes};

/// The CREATE clause arm: charge the driving rows, resolve graph-sensitive
/// patterns per row, create, and refresh the bindings.
pub(super) fn exec_create(
    graph: &ExecGraph<'_>,
    rows: Vec<Row>,
    patterns: &[relate_ir::PathPattern],
    params: &BTreeMap<String, Value>,
    qctx: QueryCtx<'_>,
) -> Result<Vec<Row>> {
    let mut rows = rows;

    // MATCH (and any earlier clause) drops its row charge on
    // return, but this clause keeps that vector until it
    // finishes. Bill it for the whole write, and again as
    // created entities land in the rows.
    let mut driving = relate_core::tokens::WorkspaceCharge::retain_with(|| {
        crate::workspace::row_buffer_bytes(&rows)
    })?;
    // Hoist the graph-sensitive decision to the pattern source: the
    // prepared patterns (when needed) are built before any write lands,
    // then one loop creates every row.
    let prepared: Option<Vec<Vec<relate_ir::PathPattern>>> =
        if patterns_have_graph_sensitive_expr(patterns) {
            Some(
                rows.iter()
                    .map(|row| {
                        resolve_create_patterns(graph, row, patterns, params, qctx.cypher_version)
                    })
                    .collect::<Result<Vec<_>>>()?,
            )
        } else {
            None
        };
    for n in 0..rows.len() {
        let row_patterns = prepared.as_ref().map_or(patterns, |p| p[n].as_slice());
        create_patterns(
            graph,
            &mut rows[n],
            row_patterns,
            params,
            qctx.cypher_version,
        )?;
        if n % 64 == 63 {
            driving.resize_with(|| crate::workspace::row_buffer_bytes(&rows))?;
        }
    }
    driving.resize_with(|| crate::workspace::row_buffer_bytes(&rows))?;
    refresh_rows(graph, &mut rows);
    driving.resize_with(|| crate::workspace::row_buffer_bytes(&rows))?;
    Ok(rows)
}

/// The MERGE clause arm: match the pattern per row, create-and-set on a
/// miss, set on a hit, then refresh the bindings.
pub(super) fn exec_merge(
    graph: &ExecGraph<'_>,
    rows: Vec<Row>,
    pattern: &relate_ir::PathPattern,
    on_create: &[relate_ir::SetItem],
    on_match: &[relate_ir::SetItem],
    params: &BTreeMap<String, Value>,
    qctx: QueryCtx<'_>,
) -> Result<Vec<Row>> {
    let mut rows = rows;

    // The match/null-check constraints drop MERGE's own self-referencing
    // tautologies; the create path keeps the original pattern (see
    // `merge_match_pattern`).
    let match_pattern = merge_match_pattern(pattern);
    let mut next = Vec::new();
    for row in rows {
        reject_null_merge_props(graph, &row, &match_pattern, params, qctx.cypher_version)?;
        let none = Residual::empty();
        let matched = match_paths(
            ExecCtx::new(graph, params, qctx.cypher_version),
            std::slice::from_ref(&match_pattern),
            MatchMode::DifferentRelationships,
            row.clone(),
            &none,
            None,
            None,
        )?;
        if matched.is_empty() {
            let mut created = row;
            create_patterns(
                graph,
                &mut created,
                std::slice::from_ref(pattern),
                params,
                qctx.cypher_version,
            )?;
            apply_sets(graph, &mut created, on_create, params, qctx.cypher_version)?;
            next.push(created);
        } else {
            for mut m in matched {
                apply_sets(graph, &mut m, on_match, params, qctx.cypher_version)?;
                next.push(m);
            }
        }
    }
    rows = next;
    refresh_rows(graph, &mut rows);
    Ok(rows)
}

/// The DELETE clause arm: resolve the expressions, delete the values, and
/// refresh the surviving bindings as tombstones.
pub(super) fn exec_delete(
    graph: &ExecGraph<'_>,
    rows: Vec<Row>,
    detach: bool,
    expressions: &[Expr],
    params: &BTreeMap<String, Value>,
    qctx: QueryCtx<'_>,
) -> Result<Vec<Row>> {
    let mut rows = rows;

    let resolved = rows
        .iter()
        .flat_map(|row| {
            expressions
                .iter()
                .map(|expr| eval(graph, row, expr, params, qctx.cypher_version))
        })
        .collect::<Result<Vec<_>>>()?;
    delete_values(graph, &resolved, detach)?;
    // The deleted binding stays addressable as an empty
    // tombstone (`refresh_rows`), which a later `MATCH` on the
    // same variable still matches (`node_ok`'s snapshot path)
    // while the store no longer holds the node.
    refresh_rows(graph, &mut rows);
    Ok(rows)
}

/// The SET clause arm: apply each item per row and refresh the bindings.
pub(super) fn exec_set(
    graph: &ExecGraph<'_>,
    rows: Vec<Row>,
    items: &[relate_ir::SetItem],
    params: &BTreeMap<String, Value>,
    qctx: QueryCtx<'_>,
) -> Result<Vec<Row>> {
    let mut rows = rows;

    for row in rows.iter_mut() {
        apply_sets(graph, row, items, params, qctx.cypher_version)?;
    }
    refresh_rows(graph, &mut rows);
    Ok(rows)
}

/// The REMOVE clause arm: resolve and apply each removal, then refresh.
pub(super) fn exec_remove(
    graph: &ExecGraph<'_>,
    rows: Vec<Row>,
    items: &[relate_ir::RemoveItem],
    params: &BTreeMap<String, Value>,
    qctx: QueryCtx<'_>,
) -> Result<Vec<Row>> {
    let mut rows = rows;

    let resolved = rows
        .iter()
        .map(|row| resolve_removes(graph, row, items, params, qctx.cypher_version))
        .collect::<Result<Vec<_>>>()?;
    for items in resolved {
        apply_resolved_removes(graph, items)?;
    }
    refresh_rows(graph, &mut rows);
    Ok(rows)
}

/// The UNWIND clause arm: one output row per list element, with the ADR-0463
/// value-buffer charge and cancellation sampling. `tail` is the clause list
/// after this UNWIND (the dispatcher slices it once for `Source::plan`).
pub(super) fn exec_unwind(
    graph: &ExecGraph<'_>,
    tail: &[Clause],
    rows: Vec<Row>,
    expression: &Expr,
    alias: &SmallName,
    params: &BTreeMap<String, Value>,
    qctx: QueryCtx<'_>,
) -> Result<Vec<Row>> {
    let mut rows = rows;

    let mut next = Vec::new();
    // ADR-0463: `next` is one retaining container — it holds every output
    // row until the UNWIND finishes. The guard owns its running total and
    // each batch's per-allocation/telemetry charge.
    let mut retained = graph.retain_bytes(0)?;
    // The suffix does not depend on the row, so the strip decision
    // is planned once and applied to each template.
    let strip = Source::plan(expression, tail);
    for row in rows {
        let v = eval(graph, &row, expression, params, qctx.cypher_version)?;
        match v {
            Value::List(xs) => {
                let mut template = row;
                if let Some(source) = &strip {
                    source.strip(&mut template);
                }
                // One per-row figure covers the pre-charge, the running
                // retained total, and the meter: each output row's value
                // buffer (width+1 slots + header) plus its slot in `next`.
                let added = crate::workspace::retained_rows_bytes(
                    xs.len(),
                    template.len().saturating_add(1),
                );
                retained.add_allocation(graph, added)?;
                for x in into_items(xs) {
                    graph.cancel_tick()?;
                    let mut r = template.clone();
                    r.insert(alias.to_string(), x);
                    next.push(r);
                }
            }
            Value::Null => {}
            other => {
                let mut r = row;
                let added = crate::workspace::retained_rows_bytes(1, r.len().saturating_add(1));
                r.insert(alias.to_string(), other);
                retained.add_allocation(graph, added)?;
                next.push(r);
            }
        }
    }
    rows = next;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use crate::exec::test_support::try_exec_cypher;
    use crate::graph::Graph;
    use relate_core::{CapacityKind, Error};

    /// ADR-0463 round 3: UNWIND materializes one clone of the carried row
    /// per list element, so its charge is `elements × (row width + 1)` slots,
    /// not one slot per element. A wide row (200 bindings) over a 30k list
    /// must hit the typed per-allocation refusal; the old element-only charge
    /// stayed far under the 256 MiB cap while the row clones allocated
    /// hundreds of MiB.
    #[test]
    fn unwinding_a_wide_row_charges_the_row_clone() {
        let width = 200usize;
        let mut query = String::from("UNWIND [1] AS seed WITH ");
        for i in 0..width {
            if i > 0 {
                query.push_str(", ");
            }
            query.push_str(&format!("seed + {i} AS a{i}"));
        }
        query.push_str(" UNWIND range(1, 30000) AS x RETURN ");
        query.push_str(
            &(0..width)
                .map(|i| format!("a{i}"))
                .collect::<Vec<_>>()
                .join(" + "),
        );
        query.push_str(" + x AS s");
        let error = try_exec_cypher(&Graph::default(), &query)
            .err()
            .expect("a wide-row UNWIND must be refused");
        assert!(
            matches!(
                error,
                Error::Capacity {
                    limit,
                    kind: CapacityKind::Expression,
                    ..
                } if limit == crate::memory::EXPRESSION_BUDGET_BYTES
            ),
            "{error}"
        );
    }
}
