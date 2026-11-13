//! Input-keyed memoisation of pure expressions and checks (LABS-1478).
//!
//! A stage asks the same question of the same input many times: on a
//! branching graph the arrival set is a small fraction of the candidate
//! stream, so a check that is a *function of its input alone* is paid once
//! per edge for one answer. [`Memo`] keeps the answers; [`Memoized`] puts
//! one in front of any [`NodePred`] without the kernel loops knowing; the
//! same pieces carry any other pure expression.
//!
//! **The purity contract** (what may be wrapped). The answer is determined
//! by the input key and by state fixed for the stage: one snapshot (a read
//! clause never sees its own writes), the statement's parameters, and a
//! bound proven row-invariant and deterministic by the decoder
//! ([`super::program::bound_is_row_invariant`]). Every
//! [`Admission`](super::program::Admission) program meets it by
//! construction. A check that reads the row (a bound-variable comparison,
//! LABS-1433's boundary pair), a nondeterministic call, or a write clause's
//! own-write view does **not**, and must never be wrapped. An error is
//! never cached: it propagates, so the first erroring input surfaces
//! exactly as it would unmemoised.
//!
//! **Hashing the input can cost more than the expression**, so the key
//! scheme is part of the API, in four tiers, cheapest first:
//! 0. no key — a value constant for the stage, computed once ([`Once`]);
//! 1. a direct-indexed table over id keys, no hashing ([`PagedBits`] for
//!    boolean answers, [`NodeValues`] for any `Copy` value);
//! 2. pointer identity of a borrowed, stage-pinned slice (a table plugs in
//!    through [`MemoTable`]; none is needed yet);
//! 3. a wide-value hash, only for expressions whose cost dwarfs it.
//!
//! [`MemoClass`] is the decode-time gate: a cheap expression is never
//! wrapped, because the probe would cost as much as the work.
//!
//! **Cost shape.** Tables follow the keys *touched*, never the row or path
//! count, and report exact bytes ([`MemoTable::heap_bytes`]). Two guards
//! keep a memo from costing more than it saves, both set by a
//! [`MemoPolicy`] per cost class: a warm-up (the first inputs of a stage
//! run unmemoised, so a small stage never allocates a table) and a
//! ceiling (past it the stage keeps answering from the function, hits
//! still served). Three safety nets bound the whole: the table's bytes are
//! charged to the retained-container meter (ADR-0463), every memo of a
//! query shares one live-bytes budget ([`MEMO_QUERY_BUDGET`]), and a memo
//! whose hit rate is below the policy floor at its one review drops its
//! table and bypasses itself. A refusal or a bypass never fails the
//! query: memoisation is an optimisation, always on.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use relate_core::{NodeId, Result};

use crate::ctx::ExecCtx;
use crate::overlay::ExecGraph;

use super::node_map::NodeMap;
use super::program::NodePred;

/// The whole query's live memo bytes, across every stage's memos (a
/// refusal freezes the asking memo, never fails the query).
pub(crate) const MEMO_QUERY_BUDGET: u64 = 16 << 20;

/// The per-statement live-bytes counter every memo of a query shares.
pub(crate) type MemoLive = Arc<AtomicU64>;

/// A memo's tuning: when it starts, when it gives up, how large it may
/// grow. The trigger follows the break-even `N = setup / (hit_rate * cost -
/// probe)`: an expression far costlier than a probe pays after a handful
/// of inputs, a store read after about a thousand. Starting values, to be
/// tuned from measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoPolicy {
    /// Inputs answered by the function before the table is first
    /// allocated: a stage that never gets this far pays one counter
    /// increment per input and nothing else.
    pub(crate) warm_up: u32,
    /// Probes after which the hit rate is reviewed once.
    pub(crate) window: u32,
    /// Below this hit rate (percent) at the review, the memo drops its
    /// table and bypasses itself: every answer was used once, so the
    /// memo costs more than it saves.
    pub(crate) min_hit_percent: u32,
    /// The table's byte ceiling for this memo.
    pub(crate) max_bytes: usize,
}

impl MemoPolicy {
    /// A store-read check (a label or property test): about 20-100 ns.
    pub(crate) const MODERATE: Self = Self {
        warm_up: 1024,
        window: 4096,
        min_hit_percent: 10,
        max_bytes: 8 << 20,
    };
    /// A microsecond-class expression (a parse, a compile, a subquery).
    #[allow(
        dead_code,
        reason = "API surface for the next consumers (LABS-1478 follow-ups); pinned by the tests below"
    )]
    pub(crate) const EXPENSIVE: Self = Self {
        warm_up: 8,
        window: 256,
        min_hit_percent: 2,
        max_bytes: 8 << 20,
    };
}

/// The decode-time verdict on whether an expression is worth a memo.
#[allow(
    dead_code,
    reason = "API surface for the next consumers (LABS-1478 follow-ups); pinned by the tests below"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoClass {
    /// Costs about what a probe costs (`id`, `size`, a string compare):
    /// never wrapped.
    Cheap,
    /// Pure, a store read per distinct key (a label or property check):
    /// memoised by input key once warm.
    PerKeyModerate,
    /// Pure and microsecond-class per distinct key (a parse, a compile, a
    /// subquery): memoised by input key almost at once.
    PerKeyExpensive,
    /// Row-invariant: one value per stage ([`Once`]), no key at all.
    ConstantPerStage,
}

impl MemoClass {
    /// The memo's tuning, `None` when the class is not keyed-memoised
    /// (cheap work is never wrapped, a constant uses [`Once`]).
    #[allow(
        dead_code,
        reason = "API surface for the next consumers (LABS-1478 follow-ups); pinned by the tests below"
    )]
    pub(crate) fn policy(self) -> Option<MemoPolicy> {
        match self {
            Self::PerKeyModerate => Some(MemoPolicy::MODERATE),
            Self::PerKeyExpensive => Some(MemoPolicy::EXPENSIVE),
            Self::Cheap | Self::ConstantPerStage => None,
        }
    }
}

/// A stage-local table from an input key to its answer.
pub(crate) trait MemoTable: Default {
    type Key: Copy;
    type Value: Copy;

    fn get(&self, key: Self::Key) -> Option<Self::Value>;
    fn insert(&mut self, key: Self::Key, value: Self::Value);
    /// The table's exact allocation, for the ceiling.
    fn heap_bytes(&self) -> usize;
}

/// Where a memo's bytes are charged: the engine's retained-container
/// meter (ADR-0463), implemented by [`ExecGraph`]. A refusal is not an
/// error to the query: the memo is an optimisation, so it stops growing.
pub(crate) trait MemoMeter {
    /// Add `added` bytes to the memo's running `total`, `Err` when the
    /// configured cap is crossed.
    fn charge(&self, total: &mut u64, added: u64) -> Result<()>;
}

impl MemoMeter for ExecGraph<'_> {
    fn charge(&self, total: &mut u64, added: u64) -> Result<()> {
        self.charge_retained(total, added)
    }
}

/// What a memo did, for PROFILE and tuning: counted after the warm-up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoStats {
    /// Inputs looked up in the table (warm-up inputs are not probes).
    pub(crate) probes: u64,
    /// Probes answered from the table.
    pub(crate) hits: u64,
    /// Answers stored.
    pub(crate) inserts: u64,
    /// Answers not stored because the meter or the ceiling refused.
    pub(crate) refused: u64,
    /// The table's allocation, bytes.
    pub(crate) bytes: u64,
    /// The memo gave up at its hit-rate review and bypasses itself.
    pub(crate) disabled: bool,
}

/// One stage's memo of a pure function over the table's keys — see the
/// module docs for the contract the function must meet.
pub(crate) struct Memo<T: MemoTable> {
    policy: MemoPolicy,
    /// Inputs answered by the function so far, up to the policy's warm-up.
    warm: u32,
    table: T,
    /// The table's running size as charged to the retained-container meter.
    retained: u64,
    /// Set once a meter or the query budget refuses: the table stops
    /// growing, hits are still served.
    frozen: bool,
    /// Set at the hit-rate review when the memo is not paying.
    disabled: bool,
    reviewed: bool,
    /// This memo's share of the query's live memo bytes, returned on drop.
    reserved: u64,
    live: Option<MemoLive>,
    stats: MemoStats,
}

impl<T: MemoTable> Default for Memo<T> {
    fn default() -> Self {
        Self::new(MemoPolicy::MODERATE, None)
    }
}

impl<T: MemoTable> Drop for Memo<T> {
    fn drop(&mut self) {
        if let Some(live) = &self.live {
            live.fetch_sub(self.reserved, Ordering::Relaxed);
        }
    }
}

/// The node-label memo: a node id to a boolean answer.
pub(crate) type NodeMemo = Memo<PagedBits>;

impl<T: MemoTable> Memo<T> {
    /// A memo under `policy`, counting its bytes against the query's
    /// shared `live` total when one is given.
    pub(crate) fn new(policy: MemoPolicy, live: Option<MemoLive>) -> Self {
        Self {
            policy,
            warm: 0,
            table: T::default(),
            retained: 0,
            frozen: false,
            disabled: false,
            reviewed: false,
            reserved: 0,
            live,
            stats: MemoStats::default(),
        }
    }

    /// `key`'s answer: the memoised one, or `compute()`'s — remembered
    /// when the stage is warm and the table is under its ceiling. An error
    /// is returned and never remembered.
    pub(crate) fn answer(
        &mut self,
        meter: &impl MemoMeter,
        key: T::Key,
        compute: impl FnOnce() -> Result<T::Value>,
    ) -> Result<T::Value> {
        if self.disabled {
            return compute();
        }
        if self.warm < self.policy.warm_up {
            self.warm += 1;
            return compute();
        }
        self.stats.probes += 1;
        if let Some(known) = self.table.get(key) {
            self.stats.hits += 1;
            return Ok(known);
        }
        let fresh = compute()?;
        self.remember(meter, key, fresh);
        self.review();
        Ok(fresh)
    }

    /// The answers for `keys`, in order, into `out` (cleared first): the
    /// misses go to `compute` together — one call with the missing keys
    /// and a buffer it fills one value per key, in order — so a batched
    /// function pays its setup once. A key repeated inside one batch is
    /// computed once per occurrence. On an error nothing is remembered.
    #[allow(
        dead_code,
        reason = "API surface for the next consumers (LABS-1478 follow-ups); pinned by the tests below"
    )]
    pub(crate) fn answer_batch(
        &mut self,
        meter: &impl MemoMeter,
        keys: &[T::Key],
        out: &mut Vec<T::Value>,
        compute: impl FnOnce(&[T::Key], &mut Vec<T::Value>) -> Result<()>,
    ) -> Result<()> {
        out.clear();
        if self.disabled {
            compute(keys, out)?;
            return Ok(());
        }
        if self.warm < self.policy.warm_up {
            self.warm = self
                .warm
                .saturating_add(keys.len().min(u32::MAX as usize) as u32);
            compute(keys, out)?;
            debug_assert_eq!(out.len(), keys.len(), "one value per key");
            return Ok(());
        }
        let misses: Vec<T::Key> = keys
            .iter()
            .copied()
            .filter(|&key| self.table.get(key).is_none())
            .collect();
        self.stats.probes += keys.len() as u64;
        self.stats.hits += (keys.len() - misses.len()) as u64;
        let mut fresh = Vec::with_capacity(misses.len());
        if !misses.is_empty() {
            compute(&misses, &mut fresh)?;
            debug_assert_eq!(fresh.len(), misses.len(), "one value per key");
            for (&key, &value) in misses.iter().zip(&fresh) {
                self.remember(meter, key, value);
            }
        }
        self.review();
        let mut fresh = fresh.into_iter();
        for &key in keys {
            out.push(match self.table.get(key) {
                Some(known) => known,
                // Past the ceiling a miss was not remembered: its value is
                // the next computed one, in key order.
                None => fresh.next().expect("one computed value per miss"),
            });
        }
        Ok(())
    }

    /// Store an answer unless the memo is frozen or at its ceiling; charge
    /// the table's growth to the meter, and freeze on a refusal (the
    /// overshoot is at most one page).
    fn remember(&mut self, meter: &impl MemoMeter, key: T::Key, value: T::Value) {
        if self.frozen || self.table.heap_bytes() >= self.policy.max_bytes {
            self.stats.refused += 1;
            return;
        }
        let before = self.table.heap_bytes();
        self.table.insert(key, value);
        self.stats.inserts += 1;
        let grew = self.table.heap_bytes().saturating_sub(before) as u64;
        if grew > 0 && meter.charge(&mut self.retained, grew).is_err() {
            self.frozen = true;
        }
        if let (true, Some(live)) = (grew > 0, &self.live) {
            self.reserved += grew;
            if live.fetch_add(grew, Ordering::Relaxed) + grew > MEMO_QUERY_BUDGET {
                self.frozen = true;
            }
        }
    }

    /// The one hit-rate review, after the policy's window of probes: a
    /// memo whose answers are mostly used once drops its table and
    /// bypasses itself.
    fn review(&mut self) {
        if self.reviewed || self.stats.probes < u64::from(self.policy.window) {
            return;
        }
        self.reviewed = true;
        let floor = u64::from(self.policy.min_hit_percent);
        if self.stats.hits * 100 < self.stats.probes * floor {
            self.table = T::default();
            self.retained = 0;
            self.disabled = true;
            if let Some(live) = &self.live {
                live.fetch_sub(self.reserved, Ordering::Relaxed);
            }
            self.reserved = 0;
        }
    }

    /// What the memo did so far, with the table's current allocation.
    #[allow(
        dead_code,
        reason = "read by PROFILE once the metric is threaded (LABS-1478)"
    )]
    pub(crate) fn stats(&self) -> MemoStats {
        MemoStats {
            bytes: self.table.heap_bytes() as u64,
            disabled: self.disabled,
            ..self.stats
        }
    }

    /// The table's exact allocation.
    #[cfg(test)]
    fn heap_bytes(&self) -> usize {
        self.table.heap_bytes()
    }
}

/// Tier 0: a value constant for the stage, computed on first use. An
/// error is returned and the next ask computes again.
#[allow(
    dead_code,
    reason = "API surface for the next consumers (LABS-1478 follow-ups); pinned by the tests below"
)]
#[derive(Default)]
pub(crate) struct Once<V: Copy>(Option<V>);

impl<V: Copy> Once<V> {
    #[allow(
        dead_code,
        reason = "API surface for the next consumers (LABS-1478 follow-ups); pinned by the tests below"
    )]
    pub(crate) fn get_or_init(&mut self, compute: impl FnOnce() -> Result<V>) -> Result<V> {
        if let Some(known) = self.0 {
            return Ok(known);
        }
        let fresh = compute()?;
        self.0 = Some(fresh);
        Ok(fresh)
    }
}

/// Tier 1: any `Copy` value per node id, in the frontier family's sparse
/// map (open addressed, hashed by one multiply).
#[allow(
    dead_code,
    reason = "API surface for the next consumers (LABS-1478 follow-ups); pinned by the tests below"
)]
pub(crate) struct NodeValues<V>(NodeMap<V>);

impl<V> Default for NodeValues<V> {
    fn default() -> Self {
        Self(NodeMap::default())
    }
}

impl<V: Copy> MemoTable for NodeValues<V> {
    type Key = NodeId;
    type Value = V;

    fn get(&self, key: NodeId) -> Option<V> {
        self.0.get(key).copied()
    }

    fn insert(&mut self, key: NodeId, value: V) {
        self.0.insert(key, value);
    }

    fn heap_bytes(&self) -> usize {
        self.0.heap_bytes()
    }
}

/// Ids per page: 4096, so a page is two 64-word planes (1 KiB).
const PAGE_BITS: u32 = 12;
const PAGE_WORDS: usize = 1 << (PAGE_BITS - 6);
/// Directory entries allowed (ids below 2^28): a larger id is simply not
/// remembered, so a sparse far id cannot grow the directory.
const MAX_PAGES: usize = 1 << 16;

/// One page: which ids are answered, and what the answer is.
struct Page {
    known: [u64; PAGE_WORDS],
    value: [u64; PAGE_WORDS],
}

/// Tier 1: a paged bitmap of boolean answers, 64 node ids per word, two
/// planes (known, value). Pages allocate on first touch, so memory follows
/// the id ranges touched (1 KiB per 4096-id range), never the row or path
/// count; a probe is a directory load and a bit test — no hashing.
#[derive(Default)]
pub(crate) struct PagedBits {
    pages: Vec<Option<Box<Page>>>,
    allocated: usize,
}

impl PagedBits {
    fn split(node: NodeId) -> Option<(usize, usize, u64)> {
        let id = u64::try_from(node).ok()?;
        let page = usize::try_from(id >> PAGE_BITS).ok()?;
        let word = ((id >> 6) as usize) & (PAGE_WORDS - 1);
        (page < MAX_PAGES).then_some((page, word, 1u64 << (id & 63)))
    }
}

impl MemoTable for PagedBits {
    type Key = NodeId;
    type Value = bool;

    fn get(&self, node: NodeId) -> Option<bool> {
        let (page, word, bit) = Self::split(node)?;
        let page = self.pages.get(page)?.as_ref()?;
        (page.known[word] & bit != 0).then(|| page.value[word] & bit != 0)
    }

    fn insert(&mut self, node: NodeId, answer: bool) {
        let Some((page, word, bit)) = Self::split(node) else {
            return;
        };
        if page >= self.pages.len() {
            self.pages.resize_with(page + 1, || None);
        }
        let slot = &mut self.pages[page];
        if slot.is_none() {
            self.allocated += 1;
            *slot = Some(Box::new(Page {
                known: [0; PAGE_WORDS],
                value: [0; PAGE_WORDS],
            }));
        }
        let page = slot.as_mut().expect("page allocated above");
        page.known[word] |= bit;
        if answer {
            page.value[word] |= bit;
        }
    }

    fn heap_bytes(&self) -> usize {
        self.pages.capacity() * std::mem::size_of::<Option<Box<Page>>>()
            + self.allocated * std::mem::size_of::<Page>()
    }
}

/// A [`NodePred`] answered through a [`NodeMemo`]: the wrapped predicate's
/// own `admits` is the check, called at most once per distinct node once
/// the stage is warm. The wrapper is `Copy` and monomorphic like the
/// predicate it wraps — the kernel loop's per-candidate call stays static.
#[derive(Clone, Copy)]
pub(crate) struct Memoized<P>(pub(crate) P);

/// The memoised predicate's per-run state: the wrapped predicate's own
/// prepared form (lazily filled, as unmemoised) beside the memo.
pub(crate) struct MemoState<T> {
    inner: Option<T>,
    memo: NodeMemo,
}

impl<P: NodePred> NodePred for Memoized<P> {
    type Prepared = MemoState<P::Prepared>;

    fn prepare(&self, ctx: ExecCtx<'_>) -> Result<Self::Prepared> {
        // The wrapped predicate prepares itself at its first real check
        // (the lazy once-per-stage evaluation of its bound), so a stage
        // with no candidates evaluates nothing, memoised or not.
        Ok(MemoState {
            inner: None,
            memo: NodeMemo::new(MemoPolicy::MODERATE, Some(ctx.graph.memo_live())),
        })
    }

    fn admits(
        &self,
        prepared: &mut Option<Self::Prepared>,
        ctx: ExecCtx<'_>,
        node: NodeId,
    ) -> Result<bool> {
        if prepared.is_none() {
            *prepared = Some(self.prepare(ctx)?);
        }
        let state = prepared
            .as_mut()
            .expect("prepared before the memoised check");
        let MemoState { inner, memo } = state;
        memo.answer(ctx.graph, node, || self.0.admits(inner, ctx, node))
    }
}

#[cfg(test)]
mod tests {
    use relate_core::Error;

    use super::*;

    const WARMUP_CHECKS: u32 = MemoPolicy::MODERATE.warm_up;

    /// A meter with no cap.
    struct Unmetered;
    impl MemoMeter for Unmetered {
        fn charge(&self, _: &mut u64, _: u64) -> Result<()> {
            Ok(())
        }
    }

    /// A meter that refuses once the running total passes a cap.
    struct Capped(u64);
    impl MemoMeter for Capped {
        fn charge(&self, total: &mut u64, added: u64) -> Result<()> {
            *total += added;
            if *total > self.0 {
                Err(Error::Execution("over the cap".into()))
            } else {
                Ok(())
            }
        }
    }

    /// A memo past its warm-up, so the next check is a memoising one.
    fn warm() -> NodeMemo {
        let mut memo = NodeMemo::default();
        for node in 0..WARMUP_CHECKS {
            assert!(memo
                .answer(&Unmetered, i64::from(node) + 1_000_000, || Ok(true))
                .unwrap());
        }
        memo
    }

    // The warm-up: the first checks run the function every time and
    // allocate nothing.
    #[test]
    fn warm_up_never_allocates_or_remembers() {
        let mut memo = NodeMemo::default();
        let mut calls = 0;
        for _ in 0..WARMUP_CHECKS {
            memo.answer(&Unmetered, 7, || {
                calls += 1;
                Ok(true)
            })
            .unwrap();
        }
        assert_eq!(calls, WARMUP_CHECKS, "every warm-up check runs");
        assert_eq!(memo.heap_bytes(), 0, "no table during the warm-up");
    }

    // Past the warm-up each distinct node is checked once, whatever the
    // answer, and repeats are served from the table.
    #[test]
    fn warm_checks_run_once_per_distinct_node() {
        let mut memo = warm();
        let mut calls = 0;
        for round in 0..5 {
            for node in [1, 2, 3] {
                let want = node % 2 == 1;
                let got = memo
                    .answer(&Unmetered, node, || {
                        calls += 1;
                        Ok(want)
                    })
                    .unwrap();
                assert_eq!(got, want, "round {round} node {node}");
            }
        }
        assert_eq!(calls, 3, "three distinct nodes, three checks");
    }

    // An error is never remembered: the next ask runs the function again.
    #[test]
    fn an_error_is_not_cached() {
        let mut memo = warm();
        let failed = memo.answer(&Unmetered, 9, || Err(Error::Execution("boom".into())));
        assert!(failed.is_err());
        let mut calls = 0;
        let retried = memo
            .answer(&Unmetered, 9, || {
                calls += 1;
                Ok(false)
            })
            .unwrap();
        assert!(!retried);
        assert_eq!(calls, 1, "the failed node was not remembered");
    }

    // The footprint follows the distinct nodes touched, not the number of
    // checks: a million repeats over ten nodes stay a ten-entry table.
    #[test]
    fn footprint_follows_distinct_nodes_not_checks() {
        let mut memo = warm();
        let before = memo.heap_bytes();
        for i in 0..1_000_000i64 {
            memo.answer(&Unmetered, i % 10, || Ok(true)).unwrap();
        }
        let grown = memo.heap_bytes() - before.min(memo.heap_bytes());
        assert!(
            grown <= 64 * 1024,
            "ten nodes cannot grow the table by {grown} bytes"
        );
    }

    // A batch past the warm-up sends only the misses to the function, in
    // one call, and serves the rest from the table.
    #[test]
    fn batch_computes_only_the_misses_in_one_call() {
        let mut memo = warm();
        let mut out = Vec::new();
        let mut calls = Vec::new();
        let mut run = |memo: &mut NodeMemo, keys: &[i64]| {
            memo.answer_batch(&Unmetered, keys, &mut out, |misses, fresh| {
                calls.push(misses.to_vec());
                fresh.extend(misses.iter().map(|k| k % 2 == 0));
                Ok(())
            })
            .unwrap();
            out.clone()
        };
        assert_eq!(run(&mut memo, &[2, 3, 4]), [true, false, true]);
        assert_eq!(run(&mut memo, &[3, 4, 5, 6]), [false, true, false, true]);
        assert_eq!(calls, vec![vec![2, 3, 4], vec![5, 6]]);
    }

    // A batch error remembers nothing, and the warm-up batch computes all.
    #[test]
    fn batch_error_is_not_cached_and_warm_up_computes_all() {
        let mut memo = warm();
        let mut out = Vec::new();
        let failed = memo.answer_batch(&Unmetered, &[7, 8], &mut out, |_, _| {
            Err(Error::Execution("boom".into()))
        });
        assert!(failed.is_err());
        let mut asked = 0;
        memo.answer_batch(&Unmetered, &[7, 8], &mut out, |misses, fresh| {
            asked = misses.len();
            fresh.extend(misses.iter().map(|_| true));
            Ok(())
        })
        .unwrap();
        assert_eq!(asked, 2, "neither key was remembered");

        let mut cold = NodeMemo::default();
        let mut seen = 0;
        cold.answer_batch(&Unmetered, &[1, 2, 3], &mut out, |keys, fresh| {
            seen = keys.len();
            fresh.extend(keys.iter().map(|_| false));
            Ok(())
        })
        .unwrap();
        assert_eq!((seen, cold.heap_bytes()), (3, 0));
    }

    // The value table memoises any `Copy` value, not only booleans.
    #[test]
    fn value_table_memoises_any_copy_value() {
        let mut memo = Memo::<NodeValues<u32>>::default();
        for node in 0..WARMUP_CHECKS {
            memo.answer(&Unmetered, i64::from(node) + 9_000_000, || Ok(0))
                .unwrap();
        }
        let mut calls = 0;
        for _ in 0..3 {
            let got = memo
                .answer(&Unmetered, 41, || {
                    calls += 1;
                    Ok(7)
                })
                .unwrap();
            assert_eq!(got, 7);
        }
        assert_eq!(calls, 1);
    }

    // Tier 0: computed once per stage; an error is retried.
    #[test]
    fn once_computes_once_and_retries_after_an_error() {
        let mut once = Once::<u64>::default();
        assert!(once
            .get_or_init(|| Err(Error::Execution("boom".into())))
            .is_err());
        let mut calls = 0;
        for _ in 0..3 {
            let got = once
                .get_or_init(|| {
                    calls += 1;
                    Ok(99)
                })
                .unwrap();
            assert_eq!(got, 99);
        }
        assert_eq!(calls, 1);
    }

    // Only a per-key-expensive expression gets the keyed memo.
    #[test]
    fn only_per_key_expensive_is_keyed() {
        assert!(MemoClass::PerKeyExpensive.policy().is_some());
        assert!(MemoClass::PerKeyModerate.policy().is_some());
        assert!(MemoClass::Cheap.policy().is_none());
        assert!(MemoClass::ConstantPerStage.policy().is_none());
    }

    // The bitmap's bytes, measured: clustered ids cost 1 KiB per 4096-id
    // page plus the directory; scattered ids flatten at the page count.
    #[test]
    fn paged_bits_bytes_follow_the_pages_touched() {
        let mut clustered = PagedBits::default();
        for id in 0..4096 {
            clustered.insert(id, true);
        }
        assert_eq!(
            clustered.heap_bytes(),
            1024 + clustered.pages.capacity() * 8
        );
        let mut scattered = PagedBits::default();
        for id in 0..4096i64 {
            scattered.insert(id * 4096, id % 2 == 0);
        }
        assert_eq!(scattered.get(4096 * 3), Some(false));
        assert_eq!(scattered.get(4096 * 4), Some(true));
        assert_eq!(scattered.get(1), None);
        assert!(scattered.heap_bytes() <= 4096 * 1024 + 4096 * 8);
    }

    // A refusing meter freezes the table: no error reaches the caller,
    // hits are still served, new answers are computed and not stored.
    #[test]
    fn a_refusing_meter_freezes_the_table_without_failing() {
        let mut memo = Memo::<PagedBits>::new(
            MemoPolicy {
                warm_up: 0,
                window: u32::MAX,
                ..MemoPolicy::MODERATE
            },
            None,
        );
        let meter = Capped(1100); // page + directory fit, a second page is refused
        let mut calls = 0;
        // Fill the first page (ids 0..4096), then touch a second page.
        for id in 0..4096i64 {
            memo.answer(&meter, id, || Ok(true)).unwrap();
        }
        assert!(!memo.frozen, "the first page fit under the cap");
        memo.answer(&meter, 9000, || Ok(true)).unwrap();
        assert!(memo.frozen, "the second page crossed the cap");
        // Frozen: a new key is computed every time, an old key still hits.
        for _ in 0..3 {
            memo.answer(&meter, 9001, || {
                calls += 1;
                Ok(true)
            })
            .unwrap();
        }
        assert_eq!(calls, 3);
        assert!(memo
            .answer(&meter, 5, || panic!("a stored key must hit"))
            .unwrap());
        assert!(memo.stats().refused >= 3);
    }

    // The counters tell PROFILE what happened.
    #[test]
    fn stats_count_probes_hits_inserts_and_bytes() {
        let mut memo = warm();
        let base = memo.stats();
        for _ in 0..4 {
            for node in [10, 11, 12] {
                memo.answer(&Unmetered, node, || Ok(true)).unwrap();
            }
        }
        let s = memo.stats();
        assert_eq!(s.probes - base.probes, 12);
        assert_eq!(s.hits - base.hits, 9);
        assert_eq!(s.inserts - base.inserts, 3);
        assert_eq!(s.refused, 0);
        assert!(s.bytes > 0);
    }

    // A policy-driven warm-up: the expensive class starts almost at once.
    #[test]
    fn the_class_policy_sets_the_trigger() {
        let mut memo = Memo::<PagedBits>::new(MemoPolicy::EXPENSIVE, None);
        for node in 0..MemoPolicy::EXPENSIVE.warm_up {
            memo.answer(&Unmetered, i64::from(node), || Ok(true))
                .unwrap();
        }
        assert_eq!(memo.heap_bytes(), 0, "still warming");
        memo.answer(&Unmetered, 7, || Ok(true)).unwrap();
        assert!(memo.heap_bytes() > 0, "memoising after eight inputs");
        assert!(MemoClass::PerKeyModerate.policy().is_some());
        assert!(MemoClass::Cheap.policy().is_none());
    }

    // The review: a memo whose answers are used once drops its table,
    // returns its bytes, and bypasses itself; a memo that hits keeps it.
    #[test]
    fn a_memo_that_never_hits_disables_itself_and_returns_its_bytes() {
        let live: MemoLive = Default::default();
        let policy = MemoPolicy {
            warm_up: 4,
            window: 64,
            min_hit_percent: 10,
            max_bytes: 1 << 20,
        };
        let mut cold = Memo::<PagedBits>::new(policy, Some(live.clone()));
        for id in 0..200i64 {
            cold.answer(&Unmetered, id * 5000, || Ok(true)).unwrap();
        }
        let stats = cold.stats();
        assert!(stats.disabled, "no repeats: not paying");
        assert_eq!((stats.bytes, live.load(Ordering::Relaxed)), (0, 0));
        // Bypassed: the function runs every time, nothing is stored.
        let mut calls = 0;
        for _ in 0..3 {
            cold.answer(&Unmetered, 1, || {
                calls += 1;
                Ok(true)
            })
            .unwrap();
        }
        assert_eq!(calls, 3);

        let mut hot = Memo::<PagedBits>::new(policy, Some(live.clone()));
        for id in 0..200i64 {
            hot.answer(&Unmetered, id % 5, || Ok(true)).unwrap();
        }
        assert!(!hot.stats().disabled, "mostly repeats: keep it");
        assert!(live.load(Ordering::Relaxed) > 0);
        drop(hot);
        assert_eq!(
            live.load(Ordering::Relaxed),
            0,
            "dropped memo returns its bytes"
        );
    }

    // The query-wide budget freezes a memo that would cross it.
    #[test]
    fn the_query_budget_freezes_memos_in_total() {
        let live: MemoLive = Arc::new(AtomicU64::new(MEMO_QUERY_BUDGET - 100));
        let policy = MemoPolicy {
            warm_up: 0,
            ..MemoPolicy::MODERATE
        };
        let mut memo = Memo::<PagedBits>::new(policy, Some(live));
        memo.answer(&Unmetered, 1, || Ok(true)).unwrap();
        assert!(memo.frozen, "the first page crosses the shared budget");
        memo.answer(&Unmetered, 99_999, || Ok(true)).unwrap();
        assert!(memo.stats().refused >= 1);
    }
}
