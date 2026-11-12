//! Input-keyed memoisation of pure per-node checks (LABS-1478).
//!
//! A kernel stage asks the same question of the same node once per edge
//! that reaches it: on a branching graph the arrival set is a small
//! fraction of the candidate stream, so a check that is a *function of the
//! node alone* — the label conjunction, a property predicate against a
//! row-invariant bound — is paid many times for one answer. [`NodeMemo`]
//! keeps the answers; [`Memoized`] puts it in front of any [`NodePred`]
//! without the kernel loops knowing.
//!
//! **The purity contract** (what may be wrapped). The wrapped check's
//! answer is determined by the node id and by state that is fixed for the
//! stage: one snapshot (a read clause never sees its own writes), the
//! statement's parameters, and a bound proven row-invariant and
//! deterministic by the decoder ([`super::program::bound_is_row_invariant`]).
//! Every [`Admission`](super::program::Admission) program meets it by
//! construction. A check that reads the row (a bound-variable comparison,
//! LABS-1433's boundary pair) or a nondeterministic call does **not**, and
//! must never be wrapped. An error is never cached: it propagates, so the
//! first erroring candidate surfaces exactly as it would unmemoised.
//!
//! **Cost shape.** The table is the frontier family's sparse [`NodeMap`]:
//! memory follows the distinct nodes *touched*, never the row or path
//! count and never the store's node-id span, and the exact bytes are
//! readable ([`NodeMap::heap_bytes`]). Two guards keep it from costing
//! more than it saves: a warm-up (the first [`WARMUP_CHECKS`] checks of a
//! stage run unmemoised, so a small stage never allocates a table) and a
//! ceiling ([`MAX_MEMO_BYTES`]; past it the stage keeps answering from the
//! check, hits still served).
//!
//! **Generalizing.** The shape — a pure function, an input key, a
//! stage-local table, a purity proof at the decoder — is the engine's
//! expression memo in miniature. Another key (a tuple of input values) or
//! another value type reuses the same contract and the same two guards;
//! only the table and the key differ.

use relate_core::{NodeId, Result};

use crate::ctx::ExecCtx;

use super::program::NodePred;

/// Checks a stage answers before its memo table is first allocated: a
/// stage that never gets this far pays one counter increment per check and
/// nothing else.
const WARMUP_CHECKS: u32 = 4096;

/// The table's byte ceiling per stage (the slot array's exact allocation).
/// 32 MiB holds about a million distinct nodes; a stage that touches more
/// keeps its existing entries and answers the rest from the check.
const MAX_MEMO_BYTES: usize = 32 << 20;

/// One stage's memo of a pure node check — see the module docs for the
/// contract the checked function must meet.
#[derive(Default)]
pub(crate) struct NodeMemo {
    /// Checks answered by the function so far, up to [`WARMUP_CHECKS`].
    warm: u32,
    /// The answers, allocated page by page after the warm-up.
    answers: PagedBits,
}

impl NodeMemo {
    /// `node`'s answer: the memoised one, or `check()`'s — remembered when
    /// the stage is warm and the table is under its ceiling. An error is
    /// returned and never remembered.
    pub(crate) fn answer(
        &mut self,
        node: NodeId,
        check: impl FnOnce() -> Result<bool>,
    ) -> Result<bool> {
        if self.warm < WARMUP_CHECKS {
            self.warm += 1;
            return check();
        }
        if let Some(known) = self.answers.get(node) {
            return Ok(known);
        }
        let fresh = check()?;
        self.answers.insert(node, fresh);
        Ok(fresh)
    }

    /// The table's exact allocation, for the ceiling's pin.
    #[cfg(test)]
    fn heap_bytes(&self) -> usize {
        self.answers.heap_bytes()
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

/// A paged bitmap of answers, 64 node ids per word, two planes (known,
/// value). Pages allocate on first touch, so memory follows the id ranges
/// touched (1 KiB per 4096-id range), never the row or path count; a probe
/// is a directory load and a bit test — no hashing.
#[derive(Default)]
struct PagedBits {
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
            if self.heap_bytes() >= MAX_MEMO_BYTES {
                return;
            }
            self.pages.resize_with(page + 1, || None);
        }
        let slot = &mut self.pages[page];
        if slot.is_none() {
            if self.allocated * std::mem::size_of::<Page>() >= MAX_MEMO_BYTES {
                return;
            }
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

    fn prepare(&self, _: ExecCtx<'_>) -> Result<Self::Prepared> {
        // The wrapped predicate prepares itself at its first real check
        // (the lazy once-per-stage evaluation of its bound), so a stage
        // with no candidates evaluates nothing, memoised or not.
        Ok(MemoState {
            inner: None,
            memo: NodeMemo::default(),
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
        memo.answer(node, || self.0.admits(inner, ctx, node))
    }
}

#[cfg(test)]
mod tests {
    use relate_core::Error;

    use super::*;

    /// A memo past its warm-up, so the next check is a memoising one.
    fn warm() -> NodeMemo {
        let mut memo = NodeMemo::default();
        for node in 0..WARMUP_CHECKS {
            assert!(memo
                .answer(i64::from(node) + 1_000_000, || Ok(true))
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
            memo.answer(7, || {
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
                    .answer(node, || {
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
        let failed = memo.answer(9, || Err(Error::Execution("boom".into())));
        assert!(failed.is_err());
        let mut calls = 0;
        let retried = memo
            .answer(9, || {
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
            memo.answer(i % 10, || Ok(true)).unwrap();
        }
        let grown = memo.heap_bytes() - before.min(memo.heap_bytes());
        assert!(
            grown <= 64 * 1024,
            "ten nodes cannot grow the table by {grown} bytes"
        );
    }
}
