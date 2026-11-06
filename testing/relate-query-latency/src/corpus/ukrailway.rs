//! The ukrailway lane's own policy (ADR-0479's deferred fourth lane): fetch
//! the pinned dogofbrian/ukrailway data set, import it into `:memory:` with
//! the workload's own script, and answer-check every entry against the
//! workload's verified rows oracle — the same lane shape as the fraud
//! workload's (a lane-owned graph and a machine-readable oracle), where it
//! differs from the graph-benchmark-ldbc-store lanes: no scale factor (the data set has one
//! size), no parameters (the queries carry their constants inline — the
//! snb-bi lane's params machinery is not forced onto a workload with nothing
//! parameterized) and no seed policy (nothing anchors on a node id).
//!
//! The oracle is IN-HOUSE verification, not upstream output: no query text
//! exists upstream for r02/r03, so both queries and their answers were
//! verified by execution against the pinned data (2026-11-02 — r02 1248
//! routePairs / 19 changeoverStations, r03 15 hops and the station list),
//! recorded once in `oracle.json` and compared with the graph-benchmark-ldbc lane's
//! no-coercion rules.
//!
//! This module imports the corpus tree's shared vocabulary — the `graph`
//! module's execution types ([`GraphHandle`], [`Routed`], and the
//! rows-oracle route [`Routed::rows_checked`] the rows-oracle lanes
//! resolve through) and the `files` submodule's rows-oracle machinery
//! (the [`Oracle`] table type, the one loader and validation) — and the
//! crate's fetch machinery; it never imports the runner or the other
//! lanes. What the lane owns itself is the oracle path derivation, the
//! import script and its loader, and the lane's own facts.

use std::path::{Path, PathBuf};

use relate::Connection;

use super::files::{load_rows_oracle, validate_rows_oracle, Oracle};
use super::graph::{CorpusLane, GraphHandle, Routed};
use super::{CorpusQuery, Workload};
use crate::fetch::{ensure_ukrailway, PINNED_COMMIT};
use crate::{read, Result};

/// The workload's rows oracle (`workloads/ukrailway/oracle.json`): the
/// 2026-11-02 verified values, recorded once — the ONE derivation of the
/// path, shared by the loader, the build-time validation and the route's
/// miss text.
fn oracle_path() -> PathBuf {
    Workload::Ukrailway.dir().join("oracle.json")
}

/// The committed import script's path — the ONE derivation of the fact,
/// shared by the loader and every error label that names the script (never
/// re-spelled as a literal beside it).
fn import_script_path() -> PathBuf {
    Workload::Ukrailway.dir().join("import.cypher")
}

/// Execute a script's statements over `conn`, in order — the import's
/// loader: the canonical splitter (the same scan the graph-benchmark-ldbc index scripts
/// use), an incomplete script refused before anything runs. The script is
/// a whole multi-statement file (the header's `//` prose rides along; the
/// splitter keeps each statement's leading comments and the engine
/// executes them), never a workload query file — so the one-statement
/// [`crate::split_header`] does not apply here.
fn run_script(conn: &mut Connection, script: &str, what: &str) -> Result<()> {
    if !relate_splitter::input_is_complete(script) {
        return Err(format!("{what} ends in an unterminated statement"));
    }
    for statement in relate_splitter::split_statements(script) {
        conn.execute(&statement)
            .map_err(|e| format!("{what}: a statement failed: {e}"))?;
    }
    Ok(())
}

/// The lane's one graph: an in-memory store the import script loads, its
/// LOAD CSV confined to the fetched data directory (the CSV policy's
/// root, ADR-0081) — the script's sources are root-relative and resolved
/// by that policy; the lane substitutes nothing.
fn load_graph(dir: &Path) -> Result<GraphHandle> {
    let mut handle = GraphHandle::memory_with_csv_root("the ukrailway graph's store", dir)?;
    let script_path = import_script_path();
    let script = read(&script_path)?;
    run_script(
        &mut handle.conn,
        &script,
        &script_path.display().to_string(),
    )?;
    Ok(handle)
}

/// The ukrailway lane: the imported graph every entry runs on, and the
/// oracle each executed entry is answer-checked with.
pub(super) struct UkrailwayLane {
    graph: Box<GraphHandle>,
    oracle: Oracle,
}

impl UkrailwayLane {
    /// The tests' lane over a memory graph — the Box lives here once,
    /// never in a test's struct literal.
    #[cfg(test)]
    fn for_test(graph: GraphHandle, oracle: Oracle) -> Self {
        Self {
            graph: Box::new(graph),
            oracle,
        }
    }
}

impl UkrailwayLane {
    /// Build the lane: parse the oracle and validate the workload against
    /// it (both through the shared rows-oracle machinery, and both
    /// failing before any network or load — a malformed workload never
    /// fetches), ensure the pinned data set is under the cache root, and
    /// import it into the lane's `:memory:` store.
    pub(super) fn new(data_root: &Path, download: bool, queries: &[CorpusQuery]) -> Result<Self> {
        let path = oracle_path();
        let oracle = load_rows_oracle(&path)?;
        validate_rows_oracle(&oracle, queries, &path)?;
        let dir = ensure_ukrailway(
            data_root,
            &Workload::Ukrailway.dir().join("data.sha256"),
            download,
        )?;
        let graph = Box::new(load_graph(&dir)?);
        Ok(Self { graph, oracle })
    }
}

impl CorpusLane for UkrailwayLane {
    /// The report's one-line graph provenance: the lane's graph is always
    /// the same imported `:memory:` graph, so the line is a fact of the
    /// lane, never per-instance state — the pinned commit spelled once in
    /// the fetch module, its eight-character short form derived here.
    fn graph_line(&self) -> String {
        // The short form is the pin's first eight characters; the pin is
        // a full commit hash (the fetch module's test pins that shape).
        let short = &PINNED_COMMIT[..8];
        format!(
            "the UK railway graph, imported into :memory: from the fetched \
             dogofbrian/ukrailway data set (@ {short})"
        )
    }

    /// The entry's full disposition: the shared rows-oracle route
    /// ([`Routed::rows_checked`]) over this lane's own oracle file — an
    /// `ok` entry runs answer-checked against its row with no parameters
    /// (the queries carry their constants inline), a kept or pin entry
    /// resolves through the shared non-executing table, and an `ok` id
    /// with no row is refused.
    fn route<'a>(&'a mut self, q: &'a CorpusQuery) -> Result<Routed<'a>> {
        let Self { graph, oracle } = self;
        Routed::rows_checked(&mut graph.conn, None, oracle, &oracle_path(), q)
    }
}

#[cfg(test)]
mod tests {
    use super::super::files::row_of;

    use super::super::workload_queries;
    use super::*;

    #[test]
    fn the_committed_oracle_covers_every_executable_entry() {
        let queries = workload_queries(Workload::Ukrailway).unwrap();
        let oracle = load_rows_oracle(&oracle_path()).unwrap();
        validate_rows_oracle(&oracle, &queries, &oracle_path()).unwrap();
        for q in &queries {
            let expected = row_of(&oracle, &q.id, &oracle_path()).unwrap();
            // The verified values are the oracle's substance: r02's one
            // row of two counts, r03's one row of hops and the station
            // list.
            assert_eq!(
                expected.expected.rows.len(),
                1,
                "{}: one verified row",
                q.id
            );
        }
        assert_eq!(oracle.len(), 2, "r02 and r03");
    }

    /// The loader over the committed import script: its statements split
    /// complete — the loader's refusal never fires on the workload's own
    /// file. The script's LOAD CSV sources are root-relative and resolved
    /// by the store's CSV policy (ADR-0081); what executes them is the
    /// gated lane test, over the fetched data.
    #[test]
    fn the_committed_import_script_splits_into_complete_statements() {
        let script = read(&import_script_path()).unwrap();
        assert!(relate_splitter::input_is_complete(&script));
        assert!(!relate_splitter::split_statements(&script).is_empty());
    }

    /// The import's loader: statements run in order, an incomplete script
    /// is refused before anything runs.
    #[test]
    fn the_imports_statements_run_in_order() {
        let db = relate::Database::open(":memory:").unwrap();
        let mut conn = db.connect().unwrap();
        run_script(
            &mut conn,
            "CREATE (:N {i: 1});\nCREATE (:N {i: 2});",
            "the test script",
        )
        .unwrap();
        let r = conn.query("MATCH (n:N) RETURN count(n) AS c").unwrap();
        assert_eq!(r.rows[0][0], relate::Value::Int(2));
        let err = run_script(&mut conn, "CREATE (:N {i: 3}", "the test script").unwrap_err();
        assert!(err.contains("unterminated statement"), "{err}");
    }

    /// The lane routes every committed entry: both `ok` entries run with
    /// their oracle as the answer check and no parameters; a kept entry
    /// plans.
    #[test]
    fn the_lane_routes_every_committed_entry() {
        let queries = workload_queries(Workload::Ukrailway).unwrap();
        let oracle = load_rows_oracle(&oracle_path()).unwrap();
        // The lane's graph and Box are built through the one constructor
        // home — the struct literal lives only in the lane itself.
        let mut lane = UkrailwayLane::for_test(
            GraphHandle::memory("the route test's graph").unwrap(),
            oracle,
        );
        super::super::assert_routes_every_committed_entry(&mut lane, &queries);
    }

    /// The graph line names the pin: the report's provenance carries the
    /// pinned commit's short form, derived from the one spelling of the
    /// pin in the fetch module.
    #[test]
    fn the_graph_line_names_the_pinned_commit() {
        let lane = UkrailwayLane {
            graph: Box::new(GraphHandle::memory("the graph line test's graph").unwrap()),
            oracle: Oracle::new(),
        };
        assert_eq!(
            lane.graph_line(),
            "the UK railway graph, imported into :memory: from the fetched \
             dogofbrian/ukrailway data set (@ ce82229e)"
        );
    }
}
