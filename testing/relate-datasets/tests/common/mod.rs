//! Helpers shared across the `relate-datasets` integration-test
//! crates (each file directly under `tests/` is its own crate; this
//! module is the single source for the helpers they share).
//!
//! Each including test crate uses a different subset of these helpers
//! (`params` alone in the dataset tests, `assert_worker_report` alone in
//! the loadtest tests), so the ones it does not use would warn as dead
//! code — allow it module-wide.
#![allow(dead_code)]

use std::collections::BTreeMap;

use relate::Value;
use relate_datasets::loadtest::{LoadtestReport, Phase};

/// Builds the parameter map for one statement from `(name, value)` pairs.
pub fn params(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> BTreeMap<String, Value> {
    pairs
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

/// Asserts the worker-run shape of a multi-worker loadtest report: the
/// configured worker count is `workers`, `ops` measured and `warmup` warmup
/// op records exist, every measured op ok, one sequence number per record
/// (`1..=ops + warmup`), and ops from at least two of the `workers` worker
/// ids.
pub fn assert_worker_report(report: &LoadtestReport, workers: u32, ops: usize, warmup: usize) {
    assert_eq!(report.config.workers, workers as usize);
    let measured_ops: Vec<_> = report
        .ops
        .iter()
        .filter(|o| o.phase == Phase::Measured)
        .collect();
    let warmup_ops: Vec<_> = report
        .ops
        .iter()
        .filter(|o| o.phase == Phase::Warmup)
        .collect();
    assert_eq!(measured_ops.len(), ops);
    assert_eq!(warmup_ops.len(), warmup);
    assert!(
        measured_ops.iter().all(|o| o.ok),
        "ops failed: {:?}",
        measured_ops.iter().filter(|o| !o.ok).collect::<Vec<_>>()
    );
    let mut seqs: Vec<u64> = report.ops.iter().map(|o| o.seq).collect();
    seqs.sort_unstable();
    assert_eq!(seqs, (1..=((ops + warmup) as u64)).collect::<Vec<_>>());
    let seen: std::collections::HashSet<u32> = report.ops.iter().map(|o| o.worker).collect();
    assert!(
        seen.len() >= 2,
        "expected more than one worker to run ops, got {seen:?}"
    );
    assert!(seen.iter().all(|w| *w < workers));
}
