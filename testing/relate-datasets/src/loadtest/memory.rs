//! Memory-report assembly for the shared load-test harness: one RSS sample,
//! graph-attributed memory, and workload deltas.
//!
//! The platform samplers live in the workspace's canonical
//! `relate_test_support::sampling` module; the require-sampling guard
//! and byte formatting live in the crate-root [`crate::memory`] core; this
//! module only shapes load-test reports.

use relate_test_support::sampling::{process_peak_rss_bytes, process_rss_bytes};
use serde::{Deserialize, Serialize};

use super::GraphSnapshot;

/// One RSS sample. Ratios are process RSS divided by live graph counts;
/// `rss_per_unit` divides by the workload's unit (account/user).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MemoryPoint {
    pub rss_bytes: u64,
    pub peak_rss_bytes: u64,
    #[serde(default)]
    pub rss_per_node: f64,
    #[serde(default)]
    pub rss_per_relationship: f64,
    #[serde(default)]
    pub rss_per_entity: f64,
    #[serde(default)]
    pub rss_per_unit: f64,
}

/// Graph-attributed RSS after load (`start.rss - after_open.rss`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GraphMemory {
    pub rss_bytes: u64,
    pub bytes_per_node: f64,
    pub bytes_per_relationship: f64,
    pub bytes_per_entity: f64,
    /// Graph RSS divided by the workload's unit (accounts/users).
    pub bytes_per_unit: f64,
}

/// RSS change from after-load to after-ops vs entities added by the workload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MemoryWorkloadDelta {
    pub rss_bytes: i64,
    pub nodes: i64,
    pub relationships: i64,
    pub rss_bytes_per_new_entity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MemoryReport {
    /// After `Database::open`, before loading the generated graph.
    pub after_open: MemoryPoint,
    /// After graph load, before warmup/measured ops.
    pub start: MemoryPoint,
    /// After measured ops.
    pub finish: MemoryPoint,
    /// `start.rss - after_open.rss` (meaningful when the URI was empty at open).
    pub graph: GraphMemory,
    pub workload_delta: MemoryWorkloadDelta,
    /// Live graph + index logical bytes after load (ADR-0042). Not process RSS.
    /// `None` when the runner cannot query the engine's logical bytes (the
    /// remote Aura reference); omitted from the report JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_used_start_bytes: Option<u64>,
    /// Live graph + index logical bytes after measured ops. `None` and omitted
    /// for a runner that cannot query the engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_used_bytes: Option<u64>,
    /// Configured logical cap; `None` when unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_limit_bytes: Option<u64>,
}

impl MemoryReport {
    /// Fill the logical-byte fields with measured values. The engine runner
    /// always has them, so `start` / `finish` are plain `u64`s stored as
    /// `Some`; a runner that cannot query the engine's logical bytes never
    /// calls this and the report honestly keeps `None` (omitted) instead of
    /// fabricated zeros.
    pub fn with_logical(mut self, start: u64, finish: u64, limit: Option<u64>) -> Self {
        self.logical_used_start_bytes = Some(start);
        self.logical_used_bytes = Some(finish);
        self.logical_limit_bytes = limit;
        self
    }
}

/// Divide `bytes` by a count, mapping a non-positive count to `0.0`.
///
/// The numerator is signed so a shrinking workload delta routes through the
/// same zero guard as the unsigned byte-count ratios.
pub(crate) fn per_count(bytes: f64, n: i64) -> f64 {
    if n <= 0 {
        0.0
    } else {
        bytes / n as f64
    }
}

/// Assemble one measurement into a [`MemoryPoint`].
///
/// `snap` `None` leaves every per-entity ratio at zero (only meaningful for
/// the sample taken before the graph is loaded).
pub(crate) fn memory_point(
    rss: u64,
    peak: u64,
    snap: Option<&GraphSnapshot>,
    unit: &str,
) -> MemoryPoint {
    let (nodes, rels, units) = snap
        .map(|s| (s.nodes, s.relationships, s.scalar(unit)))
        .unwrap_or((0, 0, 0));
    let entities = nodes.saturating_add(rels);
    let bytes = rss as f64;
    MemoryPoint {
        rss_bytes: rss,
        peak_rss_bytes: peak,
        rss_per_node: per_count(bytes, nodes),
        rss_per_relationship: per_count(bytes, rels),
        rss_per_entity: per_count(bytes, entities),
        rss_per_unit: per_count(bytes, units),
    }
}

/// Sample this process's RSS and assemble a report point.
///
/// [`run`](super::run) and every other runner call
/// [`require_sampling_available`](crate::memory::require_sampling_available)
/// before reaching this, so the report path never invents a zero-byte
/// measurement.
pub fn sample_rss(snap: Option<&GraphSnapshot>, unit: &str) -> MemoryPoint {
    let rss = process_rss_bytes().expect("RSS sampling availability is checked at run start");
    let peak = process_peak_rss_bytes().expect("RSS sampling availability is checked at run start");
    memory_point(rss, peak, snap, unit)
}

pub fn build_memory_report(
    after_open: MemoryPoint,
    start: MemoryPoint,
    finish: MemoryPoint,
    start_size: &GraphSnapshot,
    finish_size: &GraphSnapshot,
    unit: &str,
) -> MemoryReport {
    let graph_rss = start.rss_bytes.saturating_sub(after_open.rss_bytes);
    let entities = start_size.nodes.saturating_add(start_size.relationships);
    let node_delta = finish_size.nodes.saturating_sub(start_size.nodes);
    let rel_delta = finish_size
        .relationships
        .saturating_sub(start_size.relationships);
    let new_entities = node_delta.saturating_add(rel_delta);
    let rss_delta = finish.rss_bytes as i64 - start.rss_bytes as i64;
    MemoryReport {
        after_open,
        start,
        finish,
        graph: GraphMemory {
            rss_bytes: graph_rss,
            bytes_per_node: per_count(graph_rss as f64, start_size.nodes),
            bytes_per_relationship: per_count(graph_rss as f64, start_size.relationships),
            bytes_per_entity: per_count(graph_rss as f64, entities),
            bytes_per_unit: per_count(graph_rss as f64, start_size.scalar(unit)),
        },
        workload_delta: MemoryWorkloadDelta {
            rss_bytes: rss_delta,
            nodes: node_delta,
            relationships: rel_delta,
            rss_bytes_per_new_entity: per_count(rss_delta as f64, new_entities),
        },
        ..MemoryReport::default()
    }
}
