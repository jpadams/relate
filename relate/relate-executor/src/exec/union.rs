//! The UNION joiner (ADR-0344, ADR-0507): the stage's union mode derived
//! once, the fold state both part folds share, the column alignment, and
//! the first-seen dedup. Both folds — the plan-level [`fold_parts`] and
//! the subquery walker (`call::execute_query_seeded_in_scope`) — merge
//! through [`UnionFold`], so the dedup policy cannot drift between them.

use relate_core::{Error, Result, Value};
use relate_ir::Clause;
use relate_model::name_list::same_name_multiset;
use relate_planner::{check_union_joiners, LogicalPlan};

use crate::distinct_set::DistinctSet;

use super::QueryResult;

/// The stage's union mode. The planner forbids mixing `UNION` and
/// `UNION ALL` across one stage's boundaries (`InvalidClauseComposition`
/// in `relate-planner`'s validate), so one mode describes every
/// joiner of the stage.
pub(crate) enum UnionMode {
    /// `UNION ALL` (or a lone part, which has no joiner): append
    /// untouched, no hashing at all.
    All,
    /// `UNION`: every part's rows retain against one accumulating
    /// [`DistinctSet`] before appending — first-seen dedup over the
    /// concatenated streams, in input order.
    Distinct(DistinctSet),
}

/// Derive the stage's mode once from the plan's per-boundary flags. An
/// empty vector (a lone part) is [`UnionMode::All`]. A mixed vector is
/// refused with the planner's own typed error ([`check_union_joiners`]):
/// subquery-expression and conditional-branch bodies reach the fold
/// without stage validation, so it is a user-reachable shape, not an
/// invariant to assert.
pub(crate) fn union_mode(union_all: &[bool]) -> Result<UnionMode> {
    check_union_joiners(union_all)?;
    Ok(match union_all.first() {
        None | Some(true) => UnionMode::All,
        Some(false) => UnionMode::Distinct(DistinctSet::default()),
    })
}

/// Whether a part is aligned against the fold's accumulated result before
/// merging: every part is, but the arms of a scope-clause body whose
/// result never reaches a caller (their shape is arbitrary).
#[derive(Clone, Copy)]
pub(crate) enum Align {
    Compare,
    Uncompared,
}

/// The UNION fold's state: the accumulated result and the stage's mode —
/// in [`UnionMode::Distinct`], the one set every part's rows retain
/// against before appending. The joiner-state comment lives here once:
/// membership is [`crate::group_key::GroupKey`] equivalence (1 ≡ 1.0,
/// null ≡ null, every NaN distinct), the set owns its workspace charge (a
/// refused charge is a typed capacity error), and first-seen order is the
/// input order filtered by the insert results.
pub(crate) struct UnionFold {
    acc: Option<QueryResult>,
    mode: UnionMode,
}

impl UnionFold {
    pub(crate) fn new(mode: UnionMode) -> Self {
        Self { acc: None, mode }
    }

    /// Merge one part's result: align it against the accumulated result
    /// when `align` says so (never the first part — there is nothing to
    /// align against), then [`UnionMode::Distinct`] retains the part's
    /// rows against the accumulating set while [`UnionMode::All`] appends
    /// untouched.
    pub(crate) fn merge(&mut self, mut part: QueryResult, align: Align) -> Result<()> {
        if let (Some(prev), Align::Compare) = (&self.acc, align) {
            part = align_union_columns(prev, part)?;
        }
        if let UnionMode::Distinct(seen) = &mut self.mode {
            retain_first_seen(&mut part.rows, seen)?;
        }
        match &mut self.acc {
            Some(prev) => {
                prev.rows.extend(part.rows);
                prev.stats.merge(&part.stats);
            }
            None => self.acc = Some(part),
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> QueryResult {
        self.acc
            .expect("a query invariantly carries at least one part")
    }
}

pub(crate) fn fold_parts(
    plan: &LogicalPlan,
    mut run: impl FnMut(usize, &[Clause]) -> Result<QueryResult>,
) -> Result<QueryResult> {
    let mut fold = UnionFold::new(union_mode(&plan.union_all)?);
    for (i, part) in plan.parts.iter().enumerate() {
        let r = run(i, part)?;
        fold.merge(r, Align::Compare)?;
    }
    Ok(fold.finish())
}

/// Align one UNION part's result with the accumulated result's column
/// order. Neo4j matches UNION branches by column *name*, not position: a
/// branch projecting the same names in a different order
/// (`RETURN 1 AS x, 2 AS y UNION RETURN 3 AS y, 2 AS x`) is legal, and
/// its rows are reordered onto the first branch's columns. Any other
/// difference keeps the typed `UNION requires same columns` error. The one
/// spelling of that policy: both part folds (the plan-level [`fold_parts`]
/// and the subquery walker) call it, so the two cannot drift, and the name
/// comparison itself is
/// [`relate_model::name_list::same_name_multiset`], shared with the
/// parser's, front's and planner's own UNION checks.
pub(crate) fn align_union_columns(
    prev: &QueryResult,
    mut part: QueryResult,
) -> Result<QueryResult> {
    if prev.columns == part.columns {
        return Ok(part);
    }
    // A part that produced no rows may carry no columns at all: a `RETURN *`
    // over an empty input has nothing to expand its star from
    // (`WITH * WHERE false CALL () { RETURN 1 AS data } RETURN *`). Its
    // static column set was already validated by the parser, front and
    // planner checks, and aligning nothing onto `prev` is a no-op, so an
    // empty result is compatible with any shape.
    if part.rows.is_empty() && part.columns.is_empty() {
        return Ok(part);
    }
    if !same_name_multiset(&prev.columns, &part.columns) {
        return Err(Error::semantic("UNION requires same columns"));
    }
    let position: std::collections::HashMap<&str, usize> = part
        .columns
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    // Every `prev` name is present (the sorted multisets are equal and
    // column names are unique per branch), so the lookups cannot miss.
    let order: Vec<usize> = prev
        .columns
        .iter()
        .map(|name| position[name.as_str()])
        .collect();
    part.rows = part
        .rows
        .into_iter()
        .map(|row| order.iter().map(|&index| row[index].clone()).collect())
        .collect();
    part.columns = prev.columns.clone();
    Ok(part)
}

/// Keep, in order, the rows whose key `seen` has not held yet.
fn retain_first_seen(rows: &mut Vec<Vec<Value>>, seen: &mut DistinctSet) -> Result<()> {
    let offered = std::mem::take(rows);
    for row in offered {
        if seen.insert_row(&row)? {
            rows.push(row);
        }
    }
    Ok(())
}
