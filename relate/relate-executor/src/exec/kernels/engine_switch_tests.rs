//! The diagnostic engine switch's routing pins (ADR-0519): the switch's
//! three values, one suite over the fixtures its behaviors need. The
//! forced rung (`nfa`) takes a shape the cost gate declines under `auto`
//! — the engagement counter and the frontier counters both say so — the
//! stood-down kernels (`walk`) leave every clause to the per-row walker
//! on both a chain-kernel shape and a frontier shape, and the `auto`
//! twins pin that the unswitched routing is unchanged (the wide pin is
//! the existing suites, untouched: the rider's, the differentials', the
//! kernels' own).

use relate_core::Value;

use crate::exec::test_support::{exec_cypher, Builder};
use crate::graph::Graph;
use crate::pattern_engine::PatternEngine;

use super::super::take_frontier_counters;
use super::frontier_rider_tests::ring_graph;
use super::take_kernel_engagements;

/// A directed `L` line of six nodes — the bounded acyclic recursion the
/// cost gate declines (`paths ≈ endpoints`, the DFS already output-bound)
/// while the unbounded spelling of the same shape engages on cost rule
/// (a): the one fixture that separates the forced rung from `auto`.
fn line_graph() -> Graph {
    let b = Builder::new();
    let n: Vec<_> = (0..6)
        .map(|i| b.node(&format!("l{i}"), &["Line"]))
        .collect();
    for pair in n.windows(2) {
        b.rel(pair[0], pair[1], "L", &[]);
    }
    b.done()
}

/// The chain corridor (the kernels suite's `families` shape at its
/// minimal width): a fixed two-hop chain the chain kernel takes under
/// `auto`, so the stood-down rung is observable as zero engagements with
/// the walker answering.
fn corridor_graph() -> Graph {
    let b = Builder::new();
    let a1 = b.prop_node("a1", &["Alpha"], &[("k", Value::Int(1))]);
    let a2 = b.prop_node("a2", &["Alpha"], &[("k", Value::Int(2))]);
    let b1 = b.node("b1", &["Beta"]);
    let b2 = b.node("b2", &["Beta"]);
    let g1 = b.prop_node("g1", &["Gamma"], &[("name", Value::String("one".into()))]);
    let g2 = b.prop_node("g2", &["Gamma"], &[("name", Value::String("two".into()))]);
    b.rel(a1, b1, "R", &[]);
    b.rel(a2, b2, "R", &[]);
    b.rel(b1, g1, "S", &[]);
    b.rel(b1, g2, "S", &[]);
    b.rel(b2, g2, "S", &[]);
    b.done()
}

/// `nfa` forces the frontier rung onto a clause the cost gate declines
/// under `auto` — the bounded line recursion — and the forced rung
/// answers what the DFS answered. The unbounded spelling of the same
/// shape engaging under `auto` pins that the bounded decline was the
/// cost gate's, not a structural one.
#[test]
fn forced_nfa_takes_the_cost_declined_shape() {
    let bounded = "MATCH (x:Line {id:'l0'})-[:L*1..2]->(y:Line) \
                   RETURN count(DISTINCT y.id) AS c";
    let auto = exec_cypher(&line_graph(), bounded);
    assert_eq!(
        take_kernel_engagements(),
        0,
        "auto: the cost gate keeps the bounded recursion on the DFS"
    );
    assert_eq!(auto.rows, vec![vec![Value::Int(2)]]);

    let mut forced = line_graph();
    forced.configure_pattern_engine(PatternEngine::Nfa);
    let nfa = exec_cypher(&forced, bounded);
    assert_eq!(
        take_kernel_engagements(),
        1,
        "forced-nfa: the frontier rung takes the cost-declined shape"
    );
    assert!(
        take_frontier_counters().levels > 0,
        "the frontier ran for the forced clause"
    );
    assert_eq!(nfa.rows, auto.rows, "the forced rung answers the DFS");

    let unbounded = "MATCH (x:Line {id:'l0'})-[:L*]->(y:Line) \
                     RETURN count(DISTINCT y.id) AS c";
    exec_cypher(&line_graph(), unbounded);
    assert_eq!(
        take_kernel_engagements(),
        1,
        "the unbounded spelling engages under auto: the shape is frontier-eligible"
    );
}

/// `walk` stands the kernels down on both kernel families' own shapes:
/// the chain corridor the chain kernel takes under `auto`, and the
/// unbounded ring the frontier takes — zero engagements either way, the
/// per-row walker answering what the engaged rung answered.
#[test]
fn forced_walk_engages_the_per_row_walker() {
    let chain_query = "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) \
                       WHERE a.k >= 1 AND g.name CONTAINS 'o' \
                       RETURN a.k AS ak, g.name AS gn LIMIT 99";
    let mut corridor = corridor_graph();
    corridor.configure_pattern_engine(PatternEngine::Walk);
    let walked = exec_cypher(&corridor, chain_query);
    assert_eq!(
        take_kernel_engagements(),
        0,
        "walk: no kernel rung takes the chain corridor"
    );
    let auto = exec_cypher(&corridor_graph(), chain_query);
    assert_eq!(
        take_kernel_engagements(),
        1,
        "auto: the chain kernel takes the corridor"
    );
    assert_eq!(
        walked.rows, auto.rows,
        "the walker answers the chain kernel"
    );

    let frontier_query = "MATCH (x:Geo {id:'n0'})-[:LINK*]-(y:Geo) \
                          RETURN count(DISTINCT y.id) AS c";
    let mut ring = ring_graph();
    ring.configure_pattern_engine(PatternEngine::Walk);
    let walked = exec_cypher(&ring, frontier_query);
    assert_eq!(
        take_kernel_engagements(),
        0,
        "walk: no kernel rung takes the unbounded ring"
    );
    assert_eq!(
        take_frontier_counters().levels,
        0,
        "walk: the frontier never ran"
    );
    let auto = exec_cypher(&ring_graph(), frontier_query);
    assert_eq!(
        take_kernel_engagements(),
        1,
        "auto: the frontier takes the unbounded ring"
    );
    assert_eq!(walked.rows, auto.rows, "the walker answers the frontier");
}

/// The forced rung serves the interior-label gap class (class K): under
/// `auto` the compiler's plain-node decline keeps the labelled group on
/// the DFS, under `nfa` the per-state guards serve it — the answer
/// identical on both legs, the guard excluding the unlabelled tail node
/// on each. The capped spelling rides the witness reconstruction over
/// the split boundary layout the guards compile to.
#[test]
fn forced_nfa_serves_interior_labels_with_the_walkers_answer() {
    use crate::exec::test_support::same_rows;
    // The line l0..l5 (every node :Line) with an unlabelled tail: the
    // round-end guard must exclude it.
    let line_with_tail = || {
        let mut b = Builder::new();
        let line = |b: &mut Builder, name: &str| {
            b.prop_node(name, &["Line"], &[("id", Value::String(name.into()))])
        };
        let l: Vec<_> = (0..6).map(|i| line(&mut b, &format!("l{i}"))).collect();
        let tail = b.node("out", &[]);
        for pair in l.windows(2) {
            b.rel(pair[0], pair[1], "L", &[]);
        }
        b.rel(l[5], tail, "L", &[]);
        b.done()
    };
    let count = "MATCH (x:Line {id:'l0'})((m)-[:L]->(n:Line)){1,}(y) \
                 RETURN count(DISTINCT y.id) AS c";
    let auto = exec_cypher(&line_with_tail(), count);
    assert_eq!(
        take_kernel_engagements(),
        0,
        "auto: the interior-label decline keeps the labelled group on the DFS"
    );
    assert_eq!(auto.rows, vec![vec![Value::Int(5)]]);

    let mut forced = line_with_tail();
    forced.configure_pattern_engine(PatternEngine::Nfa);
    let nfa = exec_cypher(&forced, count);
    assert_eq!(
        take_kernel_engagements(),
        1,
        "forced-nfa: the per-state guards serve the labelled group"
    );
    assert!(take_frontier_counters().levels > 0);
    assert_eq!(nfa.rows, auto.rows, "the guarded rung answers the DFS");

    // The capped spelling: the frontier's reconstructed witnesses against
    // the DFS's own cap rider — the same row set on a line (one path per
    // endpoint, no multiplicity to diverge on).
    let capped = "MATCH p = (x:Line {id:'l0'})((m)-[:L]->(n:Line)){1,}(y) \
                  RETURN p LIMIT 3";
    let auto = exec_cypher(&line_with_tail(), capped);
    assert_eq!(take_kernel_engagements(), 0);
    let mut forced = line_with_tail();
    forced.configure_pattern_engine(PatternEngine::Nfa);
    let served = exec_cypher(&forced, capped);
    assert_eq!(take_kernel_engagements(), 1);
    assert!(
        same_rows(&auto, &served),
        "the capped emission answers the DFS's cap rider: \
         auto {:#?} vs forced {:#?}",
        auto.rows,
        served.rows
    );
}
