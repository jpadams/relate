//! The agg-stress lane's own policy (LABS-733, the measuring net for
//! LABS-685's planned chunk-native streaming fold): open the graph-benchmark-ldbc store —
//! the same store machinery as the pathdb and snb-bi lanes — and
//! answer-check every entry against the workload's verified rows oracle.
//! The workload's five IN-HOUSE queries are synthetic aggregation-stress
//! shapes (bounded `knows` walks whose result streams are dominated by
//! rows flowing into an aggregation), so the oracle is the value this
//! build answers — verified by execution against the pinned graph-benchmark-ldbc SF0.1
//! data (2026-11-02) and recorded per scale factor in
//! `oracle-sf<SF>.json`, never an upstream answer: no upstream query of
//! this shape exists. What the lane adds over the snb-bi shape (the graph-benchmark-ldbc
//! store, no answer oracle) is exactly the rows check; what it drops is
//! the parameter table — the queries carry their constants inline, so
//! like the fraud and ukrailway lanes' literal-anchored entries, an
//! executed entry binds no parameters.
//!
//! This module imports the corpus tree's shared vocabulary — the `graph`
//! module's execution types ([`GraphHandle`], [`Routed`], and the
//! rows-oracle route [`Routed::rows_checked`] the rows-oracle lanes
//! resolve through) and the `files` submodule's rows-oracle machinery
//! (the [`Oracle`] table type, the one loader and validation, the
//! per-scale file-name convention) — and never imports the runner or the
//! other lanes: what the lane owns itself is the oracle path derivation,
//! the graph-benchmark-ldbc store and the lane's own facts.

use std::path::PathBuf;

use super::files::{load_rows_oracle, validate_rows_oracle, Oracle};
use super::graph::{CorpusLane, GraphHandle, Routed};
use super::report::graph_benchmark_ldbc_store_line;
use super::{CorpusQuery, Workload};
use crate::suite::GraphOptions;
use crate::Result;

/// The workload's rows oracle at one scale factor
/// (`workloads/agg-stress/oracle-sf<SF>.json`): the 2026-11-02 verified
/// values over the pinned graph-benchmark-ldbc SF0.1 data, recorded once per scale factor
/// — a store at another scale factor answers different rows, so it
/// resolves its own file, and a scale factor with no file fails loudly
/// at load, never reuses another scale's rows. The ONE derivation of the
/// path, shared by the loader, the build-time validation and the route's
/// miss text.
/// The agg-stress lane: the graph-benchmark-ldbc store (boxed — a store's database and
/// connection dwarf every other lane's state), the oracle each executed
/// entry is answer-checked with, and the oracle file's path (resolved
/// once at build — the route's oracle lookup names the file in its miss
/// text, never re-deriving the path per entry).
pub(super) struct AggStressLane {
    graph: Box<GraphHandle>,
    oracle: Oracle,
    oracle_path: PathBuf,
    line: String,
}

impl AggStressLane {
    /// Build the lane: parse this scale factor's oracle and validate the
    /// workload against it (both through the shared rows-oracle
    /// machinery, and both failing before any fetch or load — a
    /// malformed workload never opens the store, and a scale factor with
    /// no oracle file fails loudly here), then open the graph-benchmark-ldbc store.
    pub(super) fn new(opts: &GraphOptions, queries: &[CorpusQuery]) -> Result<Self> {
        let path = Workload::AggStress.scale_table("oracle", opts.sf);
        let oracle = load_rows_oracle(&path)?;
        validate_rows_oracle(&oracle, queries, &path)?;
        let (graph, origin) = GraphHandle::open_graph_benchmark_ldbc(opts)?;
        Ok(Self {
            graph: Box::new(graph),
            oracle,
            oracle_path: path,
            line: graph_benchmark_ldbc_store_line(origin),
        })
    }

    /// The tests' lane over a memory graph — the report-only `line`
    /// field is empty here (the lane-under-test routes, never reports),
    /// stated once at this constructor rather than fabricated per
    /// literal.
    #[cfg(test)]
    fn for_test(graph: GraphHandle, oracle: Oracle, path: PathBuf) -> Self {
        Self {
            graph: Box::new(graph),
            oracle,
            oracle_path: path,
            line: String::new(),
        }
    }
}

impl CorpusLane for AggStressLane {
    /// The report's one-line graph provenance.
    fn graph_line(&self) -> String {
        self.line.clone()
    }

    /// The entry's full disposition: the shared rows-oracle route
    /// ([`Routed::rows_checked`]) over this lane's own oracle file — an
    /// `ok` entry runs answer-checked against its row with no parameters
    /// (the queries carry their constants inline), a kept or pin entry
    /// resolves through the shared non-executing table, and an `ok` id
    /// with no row is refused.
    fn route<'a>(&'a mut self, q: &'a CorpusQuery) -> Result<Routed<'a>> {
        let Self {
            graph,
            oracle,
            oracle_path,
            ..
        } = self;
        Routed::rows_checked(&mut graph.conn, None, oracle, oracle_path, q)
    }
}

#[cfg(test)]
mod tests {
    use crate::workload::ScaleFactor;

    use super::super::files::row_of;

    use super::super::workload_queries;
    use super::*;

    #[test]
    fn the_committed_oracle_covers_every_executable_entry() {
        let queries = workload_queries(Workload::AggStress).unwrap();
        let path = Workload::AggStress.scale_table("oracle", ScaleFactor::Sf0_1);
        let oracle = load_rows_oracle(&path).unwrap();
        assert_eq!(oracle.len(), 5, "a01..a05");
        validate_rows_oracle(&oracle, &queries, &path).unwrap();
        for q in &queries {
            let expected = row_of(&oracle, &q.id, &path).unwrap();
            // The verified values are the oracle's substance: every
            // entry answers its recorded compact rows — ten per query,
            // a04's ten being its complete fold output — while the
            // workload README records each query's full group counts.
            assert_eq!(
                expected.expected.rows.len(),
                10,
                "{}: its recorded rows",
                q.id
            );
        }
    }

    /// A scale factor with no oracle file fails loudly — never silently
    /// reuses another scale factor's rows.
    #[test]
    fn a_scale_factor_without_an_oracle_file_fails_loudly() {
        // Asserted against a path that provably does not exist (a temp
        // dir), not against SF1 — a future SF1 oracle must not turn this
        // test red; the property under test is the refusal.
        let dir = tempfile::tempdir().unwrap();
        let err = load_rows_oracle(&dir.path().join("oracle-sf0.9.json")).unwrap_err();
        assert!(err.contains("oracle-sf0.9.json"), "{err}");
    }

    /// The lane routes every committed entry: each `ok` entry runs with
    /// its oracle as the answer check and no parameters (the queries
    /// carry their constants inline); a kept entry plans.
    #[test]
    fn the_lane_routes_every_committed_entry() {
        let queries = workload_queries(Workload::AggStress).unwrap();
        // The path is derived once: the lane and the oracle it was built
        // from provably name the same file.
        let path = Workload::AggStress.scale_table("oracle", ScaleFactor::Sf0_1);
        let oracle = load_rows_oracle(&path).unwrap();
        let mut lane = AggStressLane::for_test(
            GraphHandle::memory("the route test's graph").unwrap(),
            oracle,
            path,
        );
        super::super::assert_routes_every_committed_entry(&mut lane, &queries);
    }
}
