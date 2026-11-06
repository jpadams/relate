//! LABS-805: the hop/neighbor-walk counters are PROFILE-only aggregates
//! — the walk engines tally per edge into a `WalkTallies` and flush once
//! per run, and the flush lands only while a PROFILE clause arm runs —
//! so a plain statement pays no per-edge counter work and moves no
//! counter. (Test builds default the arm flag to on, so the counters'
//! other unit tests keep asserting totals on plain statements; these
//! tests disarm explicitly to pin the production gate.)

use relate_core::Value;

use super::test_support::{build, exec_cypher, profiled};
use super::{arm_store_profiling, disarm_store_profiling, take_hop_expands, take_neighbor_walks};

/// A two-hop count over a three-node chain — the LABS-780 fold's shape,
/// the hot path the per-edge TLS bump taxed.
const TWO_HOP_COUNT: &str = "MATCH (a:N)-[:E]->(b:N)-[:E]->(c:N) RETURN count(*) AS n";

#[test]
fn non_profile_runs_never_bump_the_walk_counters() {
    let built = build(&[&["N"], &["N"], &["N"]], &[(0, 1, "E"), (1, 2, "E")]);
    disarm_store_profiling();
    // Re-arm on every exit, panics included: under a serialized run the
    // tests share one thread, and a failing assert must not leave the
    // flag disarmed for the tests after this one.
    struct Rearm;
    impl Drop for Rearm {
        fn drop(&mut self) {
            arm_store_profiling();
        }
    }
    let _rearm = Rearm;
    let _ = take_hop_expands();
    let _ = take_neighbor_walks();
    let r = exec_cypher(&built.graph, TWO_HOP_COUNT);
    assert_eq!(r.rows, vec![vec![Value::Int(1)]]);
    assert_eq!(
        take_hop_expands(),
        0,
        "a disarmed (non-PROFILE) run moves no hop counter"
    );
    assert_eq!(
        take_neighbor_walks(),
        0,
        "a disarmed (non-PROFILE) run moves no walk counter"
    );
}

/// The metric tick's save/restore (LABS-805, the rust-review round): a
/// PROFILEd statement restores the arm flag it found rather than
/// disarming unconditionally — a pre-armed thread (the test builds'
/// default) stays armed after a profiled run, a pre-disarmed thread
/// stays disarmed, so one profiled statement never flips the shared
/// serial test thread's state.
#[test]
fn profiled_statement_restores_the_prior_arm_state() {
    let built = build(&[&["N"], &["N"], &["N"]], &[(0, 1, "E"), (1, 2, "E")]);
    // Pre-armed (the test-build default): the profiled run counts while
    // armed, and the flag is still armed after it — a following plain
    // statement tallies as before.
    let _ = take_neighbor_walks();
    let records = profiled(&built.graph, TWO_HOP_COUNT);
    assert!(!records.is_empty(), "the profiled run reported its clauses");
    assert!(
        take_neighbor_walks() > 0,
        "armed: the profiled walk counted its rels"
    );
    let _ = take_neighbor_walks();
    exec_cypher(&built.graph, TWO_HOP_COUNT);
    assert!(
        take_neighbor_walks() > 0,
        "still armed after the profiled run: the plain run tallies"
    );
    // Pre-disarmed: the profiled run arms and counts, and the drop
    // restores the disarmed state — a following plain statement moves
    // nothing.
    disarm_store_profiling();
    struct Rearm;
    impl Drop for Rearm {
        fn drop(&mut self) {
            arm_store_profiling();
        }
    }
    let _rearm = Rearm;
    let _ = take_neighbor_walks();
    let records = profiled(&built.graph, TWO_HOP_COUNT);
    assert!(!records.is_empty(), "the profiled run reported its clauses");
    assert!(
        take_neighbor_walks() > 0,
        "the profiled run armed its own arm and counted"
    );
    let _ = take_neighbor_walks();
    exec_cypher(&built.graph, TWO_HOP_COUNT);
    assert_eq!(
        take_neighbor_walks(),
        0,
        "disarmed state restored after the profiled run"
    );
}

#[test]
fn armed_runs_tally_the_same_totals_the_per_edge_bumps_did() {
    let built = build(&[&["N"], &["N"], &["N"]], &[(0, 1, "E"), (1, 2, "E")]);
    let _ = take_hop_expands();
    let _ = take_neighbor_walks();
    let r = exec_cypher(&built.graph, TWO_HOP_COUNT);
    assert_eq!(r.rows, vec![vec![Value::Int(1)]]);
    // The fold's walk: hop 1's anchors are the three `N` candidates —
    // their outgoing `E` rels walked are 0->1 and 1->2 (2) — and hop 2
    // continues from the accepted ends: 1->2 from b=1, none from b=2
    // (1). Three relationships walked in total.
    assert_eq!(take_neighbor_walks(), 3, "the fold walked three rels");
}
