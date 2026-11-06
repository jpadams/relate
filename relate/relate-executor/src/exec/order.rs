//! The ORDER BY family of the projection: the parked-key model
//! ([`OrderKeys`], [`park_order_keys`], [`unpark_order_keys`]), the sort
//! entry point and its top-k selection, the paging stages, and the
//! SKIP/LIMIT bound resolution — split from `project.rs` (LABS-781's
//! stellar round: the order family is the projection's sibling concern,
//! not a section of it).

use std::cmp::Ordering as CmpOrdering;
use std::collections::BTreeMap;

use relate_core::{Error, Result, Value};
use relate_ir::{Expr, Projection};

use crate::eval::{eval, is_agg_expr};
use crate::overlay::ExecGraph;
use crate::row::Row;

use super::scope::is_star_item;
use super::scope::{projection_names, projects_name, result_column};

/// Whether an `ORDER BY` key must read the projected row even when its
/// expression is verbatim a projection item.
///
/// Every key is evaluated against the projected row (see
/// [`park_order_keys`]); this decision names the shapes that may not fall
/// back to the item they match: a bare variable naming a projected column is
/// a reference to that column, a key with an aggregate call folds the group,
/// and an explicit `GROUP BY` (including `GROUP BY ALL`) establishes the
/// alias scope for every key (`GroupByAcceptance [73]`). Every other key
/// keeps the deprecated implicit-grouping resolution — one that is exactly a
/// projection item expression is answered from that item
/// (`ProjectionSubclauseDeprecation`).
fn order_key_alias_scoped(expr: &Expr, items: &[Projection], explicit_grouping: bool) -> bool {
    explicit_grouping
        || is_agg_expr(expr)
        || matches!(expr, Expr::Variable(name) if projects_name(items, name))
}

/// A `__` column-name prefix no result column starts with, so a parked sort
/// key can never overwrite — or be read back as — a user alias. The `__`
/// namespace is where internal columns live, and a user alias is allowed to
/// spell one (`AS __ord0`).
fn parked_prefix(items: &[Projection]) -> String {
    let names = projection_names(items);
    let mut prefix = String::from("__ord");
    while names.iter().any(|name| name.starts_with(&prefix)) {
        prefix.push('_');
    }
    prefix
}

/// One projection's `ORDER BY` keys bundled with the projection items they
/// resolve against, their alias-scope decisions and the parked column each
/// key is written under. Every projection path parks its keys at projection
/// time through [`park_order_keys`], so the one sort entry point reads them
/// back instead of re-evaluating anything.
pub(crate) struct OrderKeys<'a> {
    keys: &'a [relate_ir::OrderItem],
    items: &'a [Projection],
    alias_scoped: Vec<bool>,
    parked_prefix: String,
}

impl<'a> OrderKeys<'a> {
    /// The per-key scope depends only on the key expressions and the items,
    /// never on a row, so every path takes the decision once here.
    pub(crate) fn new(
        keys: &'a [relate_ir::OrderItem],
        items: &'a [Projection],
        explicit_grouping: bool,
    ) -> Self {
        Self {
            keys,
            items,
            alias_scoped: keys
                .iter()
                .map(|item| order_key_alias_scoped(&item.expr, items, explicit_grouping))
                .collect(),
            parked_prefix: parked_prefix(items),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.keys.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    fn iter(&self) -> impl Iterator<Item = &'a relate_ir::OrderItem> {
        self.keys.iter()
    }

    fn is_alias_scoped(&self, index: usize) -> bool {
        self.alias_scoped[index]
    }

    /// The parked column of key `index`.
    pub(crate) fn name(&self, index: usize) -> String {
        format!("{}{index}", self.parked_prefix)
    }
}

/// Park every `ORDER BY` key of `keys` on `projected`, the row the
/// projection produced, so the later sort reads the values back.
///
/// The projected row is the one evaluation scope: every key resolves names
/// there, so a name the projection re-binds denotes the projected value. The
/// one short-circuit is the deprecated implicit-grouping resolution — a key
/// that is not alias-scoped and whose expression is exactly one of the items
/// verbatim is answered from that item, whose value was computed against the
/// incoming row (`ProjectionSubclauseDeprecation`: `ORDER BY a.p` beside
/// `a.p AS k` keeps the incoming `a.p`, while `ORDER BY n + 2` beside
/// `n.num AS n` reads the projected `n`, as the TCK pins).
///
/// `key_value` evaluates one key against the projected row. A key that
/// cannot be evaluated is a null sort key, not a statement failure.
pub(crate) fn park_order_keys(
    keys: &OrderKeys<'_>,
    projected: &mut Row,
    key_value: impl Fn(&Expr, &Row) -> Value,
) {
    let mut parked = Vec::with_capacity(keys.len());
    for (index, item) in keys.iter().enumerate() {
        let value = match verbatim_item(keys.items, &item.expr, projected) {
            Some(value) if !keys.is_alias_scoped(index) => value,
            _ => key_value(&item.expr, projected),
        };
        parked.push(value);
    }
    for (index, value) in parked.into_iter().enumerate() {
        projected.insert(keys.name(index), value);
    }
}

/// Remove the parked `ORDER BY` columns from every row once the sort has
/// consumed them — the one strip loop, shared by every projection path.
pub(crate) fn unpark_order_keys(keys: &OrderKeys<'_>, rows: &mut [Row]) {
    for row in rows {
        for index in 0..keys.len() {
            row.shift_remove(&keys.name(index));
        }
    }
}

/// The projected item whose expression is exactly `expr`, if any — the
/// documented verbatim resolution for a non-alias-scoped key.
fn verbatim_item(items: &[Projection], expr: &Expr, row: &Row) -> Option<Value> {
    items.iter().enumerate().find_map(|(index, item)| {
        if is_star_item(item) || item.expr != *expr {
            return None;
        }
        row.get(&item.output_name(index)).cloned()
    })
}

/// The projection item each folded `ORDER BY` key answers from, or `None`
/// when a key needs more than the folded row. The fold keeps no input rows,
/// so a key is answerable only when it is one of the projection's items
/// verbatim (an aggregate item's folded value included) or a bare variable
/// naming a projected column; any other key declines the fold and the
/// grouping path answers it.
pub(crate) fn folded_order_items(
    order: &[relate_ir::OrderItem],
    items: &[Projection],
) -> Option<Vec<usize>> {
    order
        .iter()
        .map(|key| {
            items.iter().enumerate().position(|(index, item)| {
                !is_star_item(item)
                    && (item.expr == key.expr
                        || matches!(&key.expr, Expr::Variable(name)
                    if name == item.output_name(index).as_str()))
            })
        })
        .collect()
}

fn cmp_sort_keys(
    keys: &OrderKeys<'_>,
    ka: &[Value],
    ia: usize,
    kb: &[Value],
    ib: usize,
) -> CmpOrdering {
    for (i, o) in keys.iter().enumerate() {
        match ka[i].order_by_cmp(&kb[i]) {
            CmpOrdering::Equal => {}
            ord => return if o.ascending { ord } else { ord.reverse() },
        }
    }
    ia.cmp(&ib)
}

/// Apply a catalog clause's own `YIELD` paging stages (`ORDER BY`/`SKIP`/
/// `LIMIT`), in written order, to the composed rows. The stage interpreter
/// is the same one ordinary projections use for their paging sub-grammar;
/// it lives here beside those helpers so `project_catalog` stays lookup →
/// prefill → project → filter.
pub(crate) fn apply_yield_page_stages(
    graph: &ExecGraph<'_>,
    params: &BTreeMap<String, Value>,
    cypher_version: Option<u32>,
    stages: &[relate_parser::PageStage],
    output: &mut Vec<Row>,
) -> Result<()> {
    for stage in stages {
        match stage {
            relate_parser::PageStage::OrderBy(items) => {
                let order = items
                    .iter()
                    .map(|item| {
                        Ok(relate_ir::OrderItem {
                            expr: relate_front::lower_expr(&item.expr)?,
                            ascending: !item.descending,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                // The catalog rows are their own scope: a `YIELD` key names
                // an output column, so parking reads the row itself.
                let keys = OrderKeys::new(&order, &[], false);
                for row in output.iter_mut() {
                    park_order_keys(&keys, row, |expr, scope| {
                        eval(graph, scope, expr, params, cypher_version).unwrap_or(Value::Null)
                    });
                }
                sort_rows_by_parked_keys(&keys, output, None)?;
                unpark_order_keys(&keys, output);
            }
            relate_parser::PageStage::Skip(expr) => {
                let count = skip_limit_count(&eval(
                    graph,
                    &Row::new(),
                    &relate_front::lower_expr(expr)?,
                    params,
                    cypher_version,
                )?)?;
                *output = std::mem::take(output).into_iter().skip(count).collect();
            }
            relate_parser::PageStage::Limit(expr) => {
                let count = skip_limit_count(&eval(
                    graph,
                    &Row::new(),
                    &relate_front::lower_expr(expr)?,
                    params,
                    cypher_version,
                )?)?;
                output.truncate(count);
            }
        }
    }
    Ok(())
}

/// Reserve the row buffer, then sample RSS while those rows are still the
/// live workspace and before the key vector is built.
fn hold_sort_rows(rows: &[Row]) -> Result<relate_core::tokens::WorkspaceCharge> {
    let charge = relate_core::tokens::WorkspaceCharge::retain_with(|| {
        crate::workspace::row_buffer_bytes(rows)
    })?;
    relate_core::tokens::peak_hold("sort");
    Ok(charge)
}

fn hold_sort_keys<'a>(
    keys: impl IntoIterator<Item = &'a [Value]>,
) -> Result<relate_core::tokens::WorkspaceCharge> {
    let charge = relate_core::tokens::WorkspaceCharge::retain_with(|| {
        crate::workspace::sort_key_columns(keys)
    })?;
    relate_core::tokens::peak_hold("sort");
    Ok(charge)
}

/// The parked sort keys of one row, in key order. Every projection path
/// parks them by contract ([`result_column`] asserts it).
fn parked_keys(keys: &OrderKeys<'_>, row: &Row) -> Vec<Value> {
    (0..keys.len())
        .map(|index| result_column(row, &keys.name(index)).clone())
        .collect()
}

/// The one `ORDER BY` application. Every projection path parks its keys at
/// projection time (see [`park_order_keys`]), so this reads them back and
/// never evaluates an expression. `top_k` asks for the first `top_k` rows
/// only (the `LIMIT` selection; `0` empties the table).
pub(crate) fn sort_rows_by_parked_keys(
    keys: &OrderKeys<'_>,
    rows: &mut Vec<Row>,
    top_k: Option<usize>,
) -> Result<()> {
    match top_k {
        Some(0) => {
            rows.clear();
            Ok(())
        }
        Some(keep) if keep < rows.len() => {
            *rows = take_parked_top_k(keys, std::mem::take(rows), keep)?;
            Ok(())
        }
        _ => sort_all_rows(keys, rows),
    }
}

/// The full sort behind [`sort_rows_by_parked_keys`]'s unsorted case.
fn sort_all_rows(keys: &OrderKeys<'_>, rows: &mut [Row]) -> Result<()> {
    let _rows = hold_sort_rows(rows)?;
    let mut keyed: Vec<(Vec<Value>, usize)> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (parked_keys(keys, row), index))
        .collect();
    let _keys = hold_sort_keys(keyed.iter().map(|(keys, _)| keys.as_slice()))?;
    keyed.sort_by(|a, b| cmp_sort_keys(keys, &a.0, a.1, &b.0, b.1));
    let mut slot: Vec<Option<Row>> = rows
        .iter_mut()
        .map(|row| Some(std::mem::take(row)))
        .collect();
    for (dest, (_, src)) in keyed.into_iter().enumerate() {
        rows[dest] = slot[src].take().expect("row");
    }
    Ok(())
}

fn take_parked_top_k(keys: &OrderKeys<'_>, rows: Vec<Row>, keep: usize) -> Result<Vec<Row>> {
    let mut rows_charge = hold_sort_rows(&rows)?;
    let mut keyed: Vec<(Vec<Value>, usize, Row)> = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let row_keys = parked_keys(keys, &row);
            (row_keys, index, row)
        })
        .collect();
    let mut keys_charge = hold_sort_keys(keyed.iter().map(|(keys, _, _)| keys.as_slice()))?;
    keyed.select_nth_unstable_by(keep - 1, |a, b| cmp_sort_keys(keys, &a.0, a.1, &b.0, b.1));
    keyed.truncate(keep);
    keyed.sort_by(|a, b| cmp_sort_keys(keys, &a.0, a.1, &b.0, b.1));
    rows_charge
        .resize_with(|| crate::workspace::row_refs_bytes(keyed.iter().map(|(_, _, row)| row)))?;
    keys_charge.resize_with(|| {
        crate::workspace::sort_key_columns(keyed.iter().map(|(keys, _, _)| keys.as_slice()))
    })?;
    drop(keys_charge);
    drop(rows_charge);
    Ok(keyed.into_iter().map(|(_, _, row)| row).collect())
}

pub(crate) fn skip_limit_count(v: &Value) -> Result<usize> {
    // ADR-0175: every FLOAT value is rejected — including 1.0, NaN and
    // ±Infinity (Neo4j's runtime gate rejects every float, 22N03); no
    // truncation.
    let n = match v {
        Value::Int(i) => *i,
        _ => {
            return Err(Error::invalid_argument_type(
                "SKIP/LIMIT must be an integer",
            ))
        }
    };
    if n < 0 {
        return Err(Error::execution(
            "NegativeIntegerArgument: SKIP/LIMIT must be non-negative",
        ));
    }
    // The width boundary the negative guard already spells for the low
    // end: the wasm32 surface's 32-bit `usize` would silently truncate a
    // SKIP/LIMIT at or above 2^32 through `as` (a `SKIP 4294967296`
    // skipping nothing), so the cast is explicit and refused instead.
    usize::try_from(n).map_err(|_| {
        Error::execution("IntegerTooLarge: SKIP/LIMIT exceeds the platform row-index width")
    })
}
