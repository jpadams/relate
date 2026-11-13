//! The groups' accumulator state, flat: one column per aggregate site (and
//! per shared percentile pool), one slot per group in each column.
//!
//! A group's state is a slot in each column, not a separately allocated
//! per-group vector of slots: what every group shares about a site (its
//! call, whether only an `ORDER BY` key or HAVING reads it, its seen-set
//! template) lives once in the column, and a group pays only the mutable
//! state. `count(*)` is eight bytes a group; a streaming fold is one
//! [`StreamFold`] and the bytes it holds; a retaining site (`collect`, a
//! DISTINCT percentile) is one `Vec<Value>`; a pooled percentile is one
//! [`PoolState`], shared by every call over its operand. Groups are
//! appended in landing order and never removed, so a group's index is its
//! slot in every column.

use std::collections::BTreeMap;

use relate_core::{Error, ErrorKind, Result, Value};
use relate_ir::{AggregateKind, Expr};
use relate_names::SmallName;

use crate::distinct_set::DistinctSet;
use crate::eval::{eval, fold_aggregate, needs_numbers, FoldInput, Percentile, StreamFold};
use crate::overlay::{ExecGraph, RetainedTotal};
use crate::row::Row;
use crate::workspace::buffer_growth;

use super::eval::FoldedGroup;
use super::pool::{PoolSite, PoolState, SortedPool};

/// Groups whose lenient site failed to fold: the error is parked here and
/// surfaces when the site is read. Sparse — empty unless an `ORDER BY` key
/// or HAVING operand failed (see [`GroupSubclauses::having`] for the read's
/// outcome, ADR-0458 §7).
///
/// [`GroupSubclauses::having`]: super::group::GroupSubclauses
type Failed = BTreeMap<usize, Error>;

/// What one parked failure retains: its map entry, the error value and its
/// message. Billed once per site, so K sites parking one operand's error
/// pay K times.
fn parked_error_bytes(error: &Error) -> u64 {
    (size_of::<(usize, Error)>() as u64 + error.to_string().len() as u64)
        .saturating_add(BTREE_ENTRY_OVERHEAD)
}

/// Per-entry bookkeeping of a `BTreeMap` node, a conservative flat figure.
const BTREE_ENTRY_OVERHEAD: u64 = 16;

/// One aggregate call, validated: the borrowed pieces every offer and the
/// finish read.
#[derive(Clone, Copy)]
pub(super) struct Call<'e> {
    pub(super) kind: AggregateKind,
    pub(super) name: &'e SmallName,
    pub(super) operand: &'e Expr,
    /// The percentile argument of a `PercentilePair` call.
    pub(super) percentile: Option<&'e Expr>,
}

/// The operand `call` yields for `row` in `group`, when it is to be folded:
/// `None` for a group whose site already failed (nothing is evaluated any
/// more) and for a DISTINCT call's operand already seen there. The one
/// preamble every streaming and retaining offer shares.
fn admitted_operand(
    failed: &Failed,
    seen: &mut Option<Vec<DistinctSet>>,
    group: usize,
    call: &Call<'_>,
    graph: &ExecGraph<'_>,
    row: &Row,
    params: &BTreeMap<String, Value>,
) -> Result<Option<Value>> {
    if failed.contains_key(&group) {
        return Ok(None);
    }
    graph.cancel_tick()?;
    let value = eval(graph, row, call.operand, params)?;
    let admitted = seen
        .as_mut()
        .map_or(Ok(true), |seen| seen[group].insert_value(&value))?;
    Ok(admitted.then_some(value))
}

/// The one strict-raises/lenient-parks rule of an offer: a failure on a
/// lenient site (or a pool only lenient sites read) is handed back to be
/// parked, any other failure fails the statement.
fn parked(lenient: bool, offered: Result<()>) -> Result<Option<Error>> {
    match offered {
        Ok(()) => Ok(None),
        Err(error) if lenient => Ok(Some(error)),
        Err(error) => Err(error),
    }
}

/// One aggregate site's state across every group.
pub(super) enum SiteState<'e> {
    CountStar(Vec<i64>),
    /// A non-retaining aggregate: constant-size state per group,
    /// independent of the number of rows offered.
    Stream {
        call: Call<'e>,
        /// The fold a new group starts from.
        template: StreamFold,
        folds: Vec<StreamFold>,
        /// Per group, the peak payload bytes of the one value a `min`/`max`
        /// keeps, already charged.
        held: Vec<u64>,
        /// A DISTINCT call's per-group seen-set (`Some` for such a site):
        /// an operand folds only when first seen and is dropped at once.
        seen: Option<Vec<DistinctSet>>,
    },
    /// `collect` and the DISTINCT percentiles: the operands are the state,
    /// charged to the aggregation's running total as they settle.
    Retain {
        call: Call<'e>,
        vals: Vec<Vec<Value>>,
        /// Per group, how many of `vals` are already charged.
        charged: Vec<usize>,
        /// A DISTINCT call's per-group seen-set: only first-seen operands
        /// reach `vals`.
        seen: Option<Vec<DistinctSet>>,
    },
    /// A plain percentile call: its operands live in the shared pool
    /// column at this index, not here.
    Pooled {
        pool: usize,
        call: Call<'e>,
    },
    /// An arity violation (a backtick-escaped `` `count`() `` bypasses the
    /// parser and front validation): raised when a group resolves, as the
    /// row-retaining fold did, so an empty keyed input still yields no
    /// error.
    Rejected(Error),
}

/// One aggregate call site: the call's identity (the evaluator asks for its
/// value by this reference), whether only an `ORDER BY` key or HAVING reads
/// it (a fold error parks instead of failing the statement), the groups
/// whose fold already failed, and its state.
pub(super) struct SiteColumn<'e> {
    pub(super) expr: &'e Expr,
    pub(super) lenient: bool,
    failed: Failed,
    pub(super) state: SiteState<'e>,
}

struct PoolColumn<'e> {
    site: PoolSite<'e>,
    states: Vec<PoolState>,
}

/// Every group's accumulators.
pub(crate) struct AccTable<'e> {
    groups: usize,
    pools: Vec<PoolColumn<'e>>,
    sites: Vec<SiteColumn<'e>>,
}

impl<'e> SiteColumn<'e> {
    pub(super) fn new(expr: &'e Expr, lenient: bool, state: SiteState<'e>) -> Self {
        Self {
            expr,
            lenient,
            failed: Failed::new(),
            state,
        }
    }

    /// The call's percentile argument, which the finish reads from the
    /// group's seed.
    pub(super) fn percentile(&self) -> Option<&'e Expr> {
        match &self.state {
            SiteState::Retain { call, .. } | SiteState::Pooled { call, .. } => call.percentile,
            _ => None,
        }
    }

    /// Per-group bytes of this site's column slot.
    fn slot_bytes(&self) -> u64 {
        let distinct = |seen: &Option<Vec<DistinctSet>>| {
            seen.as_ref().map_or(0, |_| size_of::<DistinctSet>() as u64)
        };
        match &self.state {
            SiteState::CountStar(_) => size_of::<i64>() as u64,
            SiteState::Stream { seen, .. } => {
                (size_of::<StreamFold>() + size_of::<u64>()) as u64 + distinct(seen)
            }
            SiteState::Retain { seen, .. } => {
                (size_of::<Vec<Value>>() + size_of::<usize>()) as u64 + distinct(seen)
            }
            SiteState::Pooled { .. } | SiteState::Rejected(_) => 0,
        }
    }

    fn open_group(&mut self) {
        match &mut self.state {
            SiteState::CountStar(counts) => counts.push(0),
            SiteState::Stream {
                template,
                folds,
                held,
                seen,
                ..
            } => {
                folds.push(template.clone());
                held.push(0);
                if let Some(seen) = seen {
                    seen.push(DistinctSet::default());
                }
            }
            SiteState::Retain {
                vals,
                charged,
                seen,
                ..
            } => {
                vals.push(Vec::new());
                charged.push(0);
                if let Some(seen) = seen {
                    seen.push(DistinctSet::default());
                }
            }
            SiteState::Pooled { .. } | SiteState::Rejected(_) => {}
        }
    }

    /// Offer one input row to group `group`'s slot.
    fn offer(
        &mut self,
        group: usize,
        graph: &ExecGraph<'_>,
        row: &Row,
        params: &BTreeMap<String, Value>,
    ) -> Result<()> {
        let Self { failed, state, .. } = self;
        match state {
            SiteState::CountStar(counts) => counts[group] += 1,
            SiteState::Stream {
                call, folds, seen, ..
            } => {
                if let Some(value) =
                    admitted_operand(failed, seen, group, call, graph, row, params)?
                {
                    call.name.with_str(|name| folds[group].offer(value, name))?;
                }
            }
            SiteState::Retain {
                call, vals, seen, ..
            } => {
                if let Some(value) =
                    admitted_operand(failed, seen, group, call, graph, row, params)?
                {
                    vals[group].push(value);
                }
            }
            SiteState::Pooled { .. } | SiteState::Rejected(_) => {}
        }
        Ok(())
    }

    /// Drop what group `group`'s slot retained: the operands (or the fold)
    /// and the DISTINCT seen-set, whichever site shape holds them. A pooled
    /// site holds nothing here (its pool state releases the sample).
    fn release(&mut self, group: usize) {
        match &mut self.state {
            SiteState::Stream {
                template,
                folds,
                seen,
                ..
            } => {
                folds[group] = template.clone();
                if let Some(seen) = seen {
                    seen[group] = DistinctSet::default();
                }
            }
            SiteState::Retain {
                vals,
                charged,
                seen,
                ..
            } => {
                vals[group] = Vec::new();
                charged[group] = 0;
                if let Some(seen) = seen {
                    seen[group] = DistinctSet::default();
                }
            }
            SiteState::CountStar(_) | SiteState::Pooled { .. } | SiteState::Rejected(_) => {}
        }
    }

    /// A lenient site's operand failed for `group`: bill what the parked
    /// error retains to `total`, release what the slot kept, and park the
    /// error. The failure surfaces when the site is read.
    fn park_failure(
        &mut self,
        group: usize,
        error: Error,
        graph: &ExecGraph<'_>,
        total: &mut RetainedTotal,
    ) -> Result<()> {
        total.add_bytes(graph, parked_error_bytes(&error))?;
        self.release(group);
        self.failed.insert(group, error);
        Ok(())
    }

    fn settle(
        &mut self,
        group: usize,
        graph: &ExecGraph<'_>,
        total: &mut RetainedTotal,
    ) -> Result<()> {
        if self.failed.contains_key(&group) {
            return Ok(());
        }
        match &mut self.state {
            SiteState::Retain { vals, charged, .. } => {
                for value in &vals[group][charged[group]..] {
                    let slot_bytes = buffer_growth(
                        charged[group],
                        charged[group] + 1,
                        size_of::<Value>() as u64,
                    );
                    total.add_bytes(graph, slot_bytes)?;
                    total.add_owned(graph, value)?;
                    charged[group] += 1;
                }
            }
            SiteState::Stream { folds, held, .. } => {
                if let Some(best) = folds[group].held_value() {
                    let now = crate::workspace::owned_heap(best);
                    if now > held[group] {
                        total.add_bytes(graph, now - held[group])?;
                        held[group] = now;
                    }
                }
            }
            SiteState::CountStar(_) | SiteState::Pooled { .. } | SiteState::Rejected(_) => {}
        }
        Ok(())
    }

    /// Resolve this site for `group`, taking the group's state out.
    fn finish(
        &mut self,
        group: usize,
        graph: &ExecGraph<'_>,
        pools: &[SortedPool],
        seed: Option<&Row>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Value> {
        if let Some(error) = self.failed.get(&group) {
            return Err(error.clone());
        }
        match &mut self.state {
            SiteState::CountStar(counts) => Ok(Value::Int(counts[group])),
            SiteState::Stream { folds, .. } => {
                std::mem::replace(&mut folds[group], StreamFold::Count(0)).finish()
            }
            SiteState::Retain { call, vals, .. } => {
                finish_retained(graph, std::mem::take(&mut vals[group]), *call, seed, params)
            }
            SiteState::Pooled { pool, call } => {
                finish_pooled(graph, &pools[*pool], *call, seed, params)
            }
            SiteState::Rejected(error) => Err(error.clone()),
        }
    }
}

impl<'e> AccTable<'e> {
    pub(super) fn new(sites: Vec<SiteColumn<'e>>, pools: Vec<PoolSite<'e>>) -> Self {
        Self {
            groups: 0,
            pools: pools
                .into_iter()
                .map(|site| PoolColumn {
                    site,
                    states: Vec::new(),
                })
                .collect(),
            sites,
        }
    }

    pub(super) fn sites(&self) -> &[SiteColumn<'e>] {
        &self.sites
    }

    /// The bytes the next group's slots add: each column's buffer growth
    /// when its length moves by one (the same reservation model every
    /// retained container is charged by). Read before [`Self::open_group`].
    pub(super) fn landing_bytes(&self) -> u64 {
        let n = self.groups;
        self.pools
            .iter()
            .map(|_| size_of::<PoolState>() as u64)
            .chain(self.sites.iter().map(SiteColumn::slot_bytes))
            .filter(|stride| *stride > 0)
            .map(|stride| buffer_growth(n, n + 1, stride))
            .fold(0, u64::saturating_add)
    }

    /// Append a fresh group's slot to every column; returns its index.
    pub(super) fn open_group(&mut self) -> usize {
        for pool in &mut self.pools {
            pool.states.push(PoolState::default());
        }
        for site in &mut self.sites {
            site.open_group();
        }
        self.groups += 1;
        self.groups - 1
    }

    /// Offer one input row to every site of group `group`. A streaming site
    /// evaluates its operand and folds it into its constant-size state; a
    /// retaining site keeps the value. What a site kept is charged by
    /// [`Self::settle`], once the spent row has let go of its share; a
    /// lenient failure that parks is billed to `total` here, as it parks.
    pub(super) fn offer(
        &mut self,
        group: usize,
        graph: &ExecGraph<'_>,
        row: &Row,
        params: &BTreeMap<String, Value>,
        total: &mut RetainedTotal,
    ) -> Result<()> {
        for index in 0..self.pools.len() {
            let pool = &mut self.pools[index];
            let state = &mut pool.states[group];
            let offered = state.offer(&pool.site, graph, row, params);
            if let Some(error) = parked(pool.site.lenient, offered)? {
                // The pool's sites are all lenient: park the failure on
                // each and stop collecting.
                state.stop();
                for site in &mut self.sites {
                    if matches!(site.state, SiteState::Pooled { pool, .. } if pool == index) {
                        site.park_failure(group, error.clone(), graph, total)?;
                    }
                }
            }
        }
        for site in &mut self.sites {
            let offered = site.offer(group, graph, row, params);
            if let Some(error) = parked(site.lenient, offered)? {
                site.park_failure(group, error, graph, total)?;
            }
        }
        Ok(())
    }

    /// Charge what group `group`'s sites kept since the last settle to the
    /// aggregation's one running total: a retaining site's new operand slots
    /// and payloads, a pool's buffer growth, and the growth of the payload
    /// a `min`/`max` holds. The caller settles after the spent input row is
    /// dropped, so a value cloned from that row is the sole owner of its
    /// payload by now and is charged in full (a payload still shared with
    /// the store or a parameter adds nothing, as everywhere else). One total
    /// for every group: the groups' retained state is alive together, so it
    /// is bounded together.
    pub(super) fn settle(
        &mut self,
        group: usize,
        graph: &ExecGraph<'_>,
        total: &mut RetainedTotal,
    ) -> Result<()> {
        for pool in &mut self.pools {
            pool.states[group].settle(graph, total)?;
        }
        for site in &mut self.sites {
            site.settle(group, graph, total)?;
        }
        Ok(())
    }

    /// Resolve every site of group `group`, taking its state out. `seed` is
    /// the group's own first row — a percentile's second argument is read
    /// from it, and an empty group has no percentile to report.
    pub(super) fn finish_group(
        &mut self,
        group: usize,
        graph: &ExecGraph<'_>,
        seed: Option<&Row>,
        params: &BTreeMap<String, Value>,
    ) -> FoldedGroup<'e> {
        let pools: Vec<SortedPool> = self
            .pools
            .iter_mut()
            .map(|pool| std::mem::take(&mut pool.states[group]).finish())
            .collect();
        FoldedGroup::new(
            self.sites
                .iter_mut()
                .map(|site| {
                    let value = site.finish(group, graph, &pools, seed, params);
                    (site.expr, value)
                })
                .collect(),
        )
    }
}

/// The validated percentile argument of a call. The percentile depends only
/// on the group's first row; an empty group has no row to ask and no
/// percentile to report (`None`).
fn percentile_input(
    graph: &ExecGraph<'_>,
    call: Call<'_>,
    seed: Option<&Row>,
    params: &BTreeMap<String, Value>,
) -> Result<Option<Percentile>> {
    let percentile = call
        .percentile
        .ok_or_else(|| Error::internal("a percentile call carries its percentile argument"))?;
    let Some(first) = seed else {
        return Ok(None);
    };
    let p = eval(graph, first, percentile, params)?;
    // Two distinct rejections: a non-numeric percentile is the
    // wrong-type class, an out-of-[0, 1] one the NumberOutOfRange
    // kind (ADR-0175's 22N03 mapping).
    let Some(p) = p.as_number() else {
        return Err(Error::invalid_argument_type("percentile must be a number"));
    };
    if !(0.0..=1.0).contains(&p) {
        return Err(Error::argument_value_kind(
            ErrorKind::NumberOutOfRange,
            "percentile",
        ));
    }
    call.name
        .with_str(|text| Percentile::new(call.kind, p, text))
        .map(Some)
}

/// A retaining site's result: the percentile argument is validated first,
/// and the shared per-kind fold finishes the operands (`collect` and
/// the DISTINCT percentiles).
fn finish_retained(
    graph: &ExecGraph<'_>,
    vals: Vec<Value>,
    call: Call<'_>,
    seed: Option<&Row>,
    params: &BTreeMap<String, Value>,
) -> Result<Value> {
    let input = match call.percentile {
        None => FoldInput::Many(call.kind),
        Some(_) => match percentile_input(graph, call, seed, params)? {
            Some(percentile) => FoldInput::Percentile(percentile),
            None => return Ok(Value::Null),
        },
    };
    call.name.with_str(|text| fold_aggregate(input, text, vals))
}

/// A pooled percentile's result: the same order of rejections as the
/// per-site fold (a parked evaluation failure is already a failed site, read
/// before this; then the percentile argument, then a non-numeric operand),
/// then the rank in the sample the group sorted once.
fn finish_pooled(
    graph: &ExecGraph<'_>,
    pool: &SortedPool,
    call: Call<'_>,
    seed: Option<&Row>,
    params: &BTreeMap<String, Value>,
) -> Result<Value> {
    let Some(percentile) = percentile_input(graph, call, seed, params)? else {
        return Ok(Value::Null);
    };
    if pool.non_numeric {
        return Err(call.name.with_str(needs_numbers));
    }
    Ok(percentile.rank_in_sorted(&pool.vals))
}

#[cfg(test)]
mod tests {
    use super::super::stream::AggSites;
    use super::*;
    use crate::exec::test_support::{agg_fn_over, group_with_vals, percentile_fn};
    use crate::graph::Graph;

    fn p_of_x() -> Expr {
        Expr::Property {
            key_id: Default::default(),
            expr: Box::new(Expr::Variable("x".into())),
            key: SmallName::from_name("p"),
        }
    }

    /// A lenient site that parks a failure releases what its group slot
    /// retained, whichever shape the site has (a streaming fold, retained
    /// operands, or a shared percentile pool), and every site answers the
    /// group with the parked error.
    #[test]
    fn a_parked_failure_releases_the_slot_in_every_site_shape() {
        let g = Graph::default();
        let graph = ExecGraph::read(&g);
        let params = BTreeMap::new();
        let calls = [
            agg_fn_over("count", p_of_x()),
            agg_fn_over("collect", p_of_x()),
            percentile_fn_over_p(),
        ];
        let mut table = AggSites::discover(&graph, [], calls.iter(), &params)
            .expect("discovered")
            .into_table();
        table.open_group();
        let mut total = graph.retain_bytes(0).expect("a fresh total");
        let ok = Value::Map(BTreeMap::from([("p".to_string(), Value::Int(1))]).into());
        let rows = group_with_vals(&[ok, Value::Int(5)]);
        table
            .offer(0, &graph, &rows[0], &params, &mut total)
            .expect("offered");
        table.settle(0, &graph, &mut total).expect("settled");
        let SiteState::Retain { vals, .. } = &table.sites[1].state else {
            panic!("collect retains")
        };
        assert_eq!(vals[0].len(), 1);
        assert_eq!(table.pools[0].states[0].held(), 1);
        let before = total.total();

        // `5.p` fails on every lenient site: parked, billed, released.
        table
            .offer(0, &graph, &rows[1], &params, &mut total)
            .expect("parked");
        assert!(total.total() > before, "the parked errors are billed");
        let SiteState::Stream { folds, .. } = &table.sites[0].state else {
            panic!("count streams")
        };
        assert_eq!(
            folds[0].clone().finish().expect("a fresh count"),
            Value::Int(0)
        );
        let SiteState::Retain { vals, charged, .. } = &table.sites[1].state else {
            panic!("collect retains")
        };
        assert!(vals[0].is_empty());
        assert_eq!(charged[0], 0);
        assert_eq!(table.pools[0].states[0].held(), 0);
        assert!(table.sites.iter().all(|site| site.failed.contains_key(&0)));
        let folded = table.finish_group(0, &graph, rows.first(), &params);
        for call in &calls {
            assert!(folded.value(call).is_err(), "{call:?}");
        }
    }

    fn percentile_fn_over_p() -> Expr {
        let Expr::Function { name, distinct, .. } = percentile_fn("percentileCont", 0.5) else {
            panic!("a function call")
        };
        Expr::Function {
            name,
            distinct,
            args: vec![p_of_x(), Expr::Float(0.5)],
        }
    }
}
