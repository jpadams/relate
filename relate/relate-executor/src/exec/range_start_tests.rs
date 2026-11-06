//! ADR-0486's execution parity on the range index: the window counter
//! pin (the reads the index-selected start pays) and the decline side —
//! the one decline the range id seek and the range cardinality share,
//! an unbounded temporal posting — keeps both the ladder's price and
//! the funnel's candidates on the scan path, with the rows the scan
//! path's. The displacement pins live in `pivot_tests`, beside the
//! start-selection shapes they extend.

use super::test_support::{
    exec_autocommit, exec_cypher, range_start_graph, try_exec_cypher_params,
};
use super::*;
use chrono::NaiveDate;
use relate_core::Temporal;

/// A RANGE index whose postings carry an unbounded temporal (a stored
/// `LocalDateTime` outside the order-key window) declines the
/// comparison — the one decline the id seek and the cardinality share —
/// so the ladder leaves the window unpriced (the label-level tie keeps
/// the written `P` start) and the funnel keeps its scan tail: the index
/// changes neither the reads nor the rows, and the unbounded satisfier
/// (year -5000, unposted but `<` the bound) still matches — the
/// false-negative row a served seek would have dropped.
#[test]
fn an_unbounded_temporal_posting_keeps_the_scan_path() {
    let local_datetime = |year: i32| {
        Value::Temporal(Temporal::LocalDateTime(
            NaiveDate::from_ymd_opt(year, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        ))
    };
    // Two posted values below the bound, one unorderable (year -5000,
    // outside the order-key window — unposted, still `<` the bound), 97
    // posted above it.
    let mut values = vec![
        local_datetime(1990),
        local_datetime(1991),
        local_datetime(-5000),
    ];
    values.extend((0..97).map(|_| local_datetime(2100)));
    let g = range_start_graph("t", &values);
    let params = BTreeMap::from([("until".to_string(), local_datetime(2000))]);
    let query = "MATCH (p:P)-[:LINKS]->(q:Q) WHERE q.t < $until \
                 RETURN id(p) AS p, id(q) AS q ORDER BY p, q";
    take_node_reads();
    let scan = try_exec_cypher_params(&g, query, &params).unwrap();
    assert_eq!(
        scan.rows.len(),
        3,
        "both posted values and the unbounded one"
    );
    assert_eq!(
        take_node_reads(),
        20,
        "the label scan: one node-store read per P candidate"
    );
    exec_autocommit(&g, "CREATE INDEX FOR (q:Q) ON (q.t)").unwrap();
    take_node_reads();
    let declined = try_exec_cypher_params(&g, query, &params).unwrap();
    assert_eq!(
        take_node_reads(),
        20,
        "the index declined on its unbounded posting: the scan path is kept"
    );
    assert_eq!(
        declined.rows, scan.rows,
        "identical results, the unbounded satisfier included"
    );
}

/// ADR-0486's execution parity: the ladder-priced start draws its
/// candidates through the existing `RangeSeek::Cmp` funnel
/// (`leaf_seek`/`serve`/`lookup_range`), so the node-store reads
/// collapse from the whole-label scan the unpriced side paid (one read
/// per `P` candidate) to the index-selected work — the window's four
/// served candidates plus the four bound-start re-reads the suffix
/// hand-off pays — with the rows identical to the scan path's.
#[test]
fn a_range_window_start_counts_the_index_selected_reads() {
    let g = range_start_graph("x", &(0..100).map(Value::Int).collect::<Vec<_>>());
    let query = "MATCH (p:P)-[:LINKS]->(q:Q) WHERE q.x < 4 \
                 RETURN id(p) AS p, id(q) AS q ORDER BY p, q";
    // The scan path, before any index exists: the label-level tie keeps
    // the written `P` start, and its label scan pays one node-store read
    // per candidate.
    take_node_reads();
    let scan = exec_cypher(&g, query);
    assert_eq!(scan.rows.len(), 4);
    assert_eq!(
        take_node_reads(),
        20,
        "the unpriced start scans the whole P label: one read per candidate"
    );
    // The index prices the Q side's window (4 of 100), the ladder
    // displaces the start onto it, and the funnel serves the window.
    exec_autocommit(&g, "CREATE INDEX FOR (q:Q) ON (q.x)").unwrap();
    take_node_reads();
    let sought = exec_cypher(&g, query);
    assert_eq!(
        take_node_reads(),
        8,
        "the window's four index-served candidates plus the four bound-start re-reads the suffix hand-off pays"
    );
    assert_eq!(
        sought.rows, scan.rows,
        "the seek path's rows are the scan path's"
    );
}
