//! Early predicate evaluation inside the GPM walk (the plain-conjunct
//! twin of the allReduce inliner's exactness tests): the differential
//! contract — the early (MATCH-WHERE) run, the evaluation-disabled run of
//! the very same statement, and the WITH-WHERE post-filter spelling must
//! answer identically — the PROFILE hop counter showing the pruned walk
//! expanding strictly fewer hops (the branch abort: a failed prefix
//! enumerates no further hops), the evaluation counter showing the
//! predicate evaluated strictly fewer times than the post-hop
//! (once-per-complete-path) evaluation, and the decline rules (a rebinding
//! name, a null pre-binding, a raising conjunct) keeping the post-filter's
//! behavior exactly.

use relate_core::Value;

use super::early::{take_early_evals, with_early_predicates_disabled};
use super::qpp_hops;
use crate::exec::test_support::{exec_cypher, try_exec_cypher, Builder};
use crate::graph::Graph;

/// The branch-abort fixture: a fixed hop from `s` to one of two mid
/// nodes, then a quantified tail to `t`. `m1` fails the mid predicate and
/// carries the denser tail (four 2-hop paths), `m2` passes it with one
/// path — the early evaluation must keep the walk off `m1`'s tail
/// entirely, where the post-filter spelling walks both.
fn mid_gate_graph() -> Graph {
    let b = Builder::new();
    let s = b.node("s", &["Geo"]);
    let m1 = b.prop_node("m1", &["Geo"], &[("keep", Value::Bool(false))]);
    let m2 = b.prop_node("m2", &["Geo"], &[("keep", Value::Bool(true))]);
    let t = b.node("t", &["Geo"]);
    let u1 = b.node("u1", &["Geo"]);
    let u2 = b.node("u2", &["Geo"]);
    b.rel(s, m1, "ROAD", &[]);
    b.rel(s, m2, "ROAD", &[]);
    // m1's tail: four 2-hop paths to t (through u1 and u2, both orders).
    b.rel(m1, u1, "LINK", &[]);
    b.rel(u1, t, "LINK", &[]);
    b.rel(m1, u2, "LINK", &[]);
    b.rel(u2, t, "LINK", &[]);
    // m2's tail: one 1-hop path.
    b.rel(m2, t, "LINK", &[]);
    b.done()
}

/// The shared body of the mid-gate spellings: `filter` is the MATCH WHERE
/// (the early evaluation) or empty, and `tail` sits between the MATCH and
/// the RETURN (the WITH-WHERE post-filter spelling).
fn mid_gate(filter: &str, tail: &str) -> String {
    format!(
        "MATCH (s:Geo {{id:'s'}})-[:ROAD]->(m:Geo)-[:LINK*1..2]->(t:Geo) {filter} {tail} \
         RETURN s.id AS s, m.id AS m, t.id AS t ORDER BY s, m, t"
    )
}

/// The early spelling: the mid predicate rides the MATCH WHERE.
fn early_query() -> String {
    mid_gate("WHERE m.keep = true", "")
}

/// The post-filter spelling: the same predicate on a WITH WHERE, evaluated
/// once per complete path.
fn postfilter_query() -> String {
    "MATCH (s:Geo {id:'s'})-[:ROAD]->(m:Geo)-[:LINK*1..2]->(t:Geo) \
         WITH s, m, t WHERE m.keep = true \
         RETURN s.id AS s, m.id AS m, t.id AS t ORDER BY s, m, t"
        .to_string()
}

/// The mid predicate evaluates as soon as `m` binds — pre-hop at the
/// anchor of the quantified tail — so the walk never enumerates the
/// failing branch's tail: the PROFILE hop counter shows the early run
/// expanding strictly fewer hops than both the post-filter spelling and
/// the evaluation-disabled run of the very same statement (which expand
/// identically — only the early evaluation prunes).
#[test]
fn hop_counter_shows_the_failed_prefix_enumerating_no_further_hops() {
    let graph = mid_gate_graph();
    let early = qpp_hops(&graph, &early_query());
    let post = qpp_hops(&graph, &postfilter_query());
    let disabled = with_early_predicates_disabled(|| qpp_hops(&graph, &early_query()));
    eprintln!("HOPS early={early} post={post} disabled={disabled}");
    assert!(early > 0, "the quantified tail counts its hops");
    assert!(early < post, "early {early} hops vs post-filter {post}");
    assert!(early < disabled, "early {early} vs disabled {disabled}");
    assert_eq!(post, disabled, "the post-filter form never evaluates early");
}

/// The mid predicate is evaluated once per mid-node binding (twice on the
/// fixture) instead of once per complete path (five paths without the
/// predicate) — strictly fewer evaluations than the post-hop evaluation,
/// the counter form of the capability's acceptance.
#[test]
fn early_evaluations_are_strictly_fewer_than_posthop_evaluations() {
    let graph = mid_gate_graph();
    // The complete-path count the post-filter would evaluate over: every
    // path the predicate-less walk yields (m1's four plus m2's one).
    let unfiltered = exec_cypher(&graph, &mid_gate("", "")).rows;
    assert_eq!(
        unfiltered.len(),
        5,
        "the fixture's path count: {unfiltered:?}"
    );
    take_early_evals();
    exec_cypher(&graph, &early_query());
    let evals = take_early_evals();
    eprintln!("EVALS early={evals} paths={}", unfiltered.len());
    assert!(
        evals < unfiltered.len() as u64,
        "early {evals} evaluations vs {} post-hop evaluations",
        unfiltered.len()
    );
    assert_eq!(evals, 2, "once per mid-node binding: {evals}");
}

/// Differential: the early evaluation answers exactly what the post-filter
/// spelling and the evaluation-disabled run answer, on the mid-gate
/// fixture and on the allReduce tests' cyclic ring (a mid predicate over
/// the boundary between two quantified groups).
#[test]
fn early_answers_match_the_postfilter_on_both_fixtures() {
    let ring_early = "MATCH (a:Geo {id:'c0'}) (()-[r1:ROAD]-(x:Geo)){1,2} (m:Geo) \
         (()-[r2:ROAD]-(y:Geo)){1,2} (z:Geo {id:'c5'}) \
         WHERE m.id = 'c3' \
         RETURN a.id AS a, m.id AS m, z.id AS z ORDER BY a, m, z";
    let ring_post = "MATCH (a:Geo {id:'c0'}) (()-[r1:ROAD]-(x:Geo)){1,2} (m:Geo) \
         (()-[r2:ROAD]-(y:Geo)){1,2} (z:Geo {id:'c5'}) \
         WITH a, m, z WHERE m.id = 'c3' \
         RETURN a.id AS a, m.id AS m, z.id AS z ORDER BY a, m, z";
    let cases: [(Graph, String, String); 2] = [
        (mid_gate_graph(), early_query(), postfilter_query()),
        (
            super::allreduce_inline_tests::ev_ring_graph(),
            ring_early.to_string(),
            ring_post.to_string(),
        ),
    ];
    for (graph, early, postfilter) in cases {
        let early_rows = exec_cypher(&graph, &early).rows;
        assert_eq!(
            early_rows,
            exec_cypher(&graph, &postfilter).rows,
            "early vs post-filter"
        );
        assert_eq!(
            early_rows,
            with_early_predicates_disabled(|| exec_cypher(&graph, &early).rows),
            "early vs evaluation-disabled"
        );
    }
}

/// A conjunct over a rebinding name (the quantified group's rel variable,
/// whose completed value is the accumulated list) is declined: it stays a
/// post-filter, evaluated zero times early, and answers identically.
#[test]
fn rebinding_names_are_declined_to_the_postfilter() {
    let graph = mid_gate_graph();
    let early = "MATCH (s:Geo {id:'s'})-[:ROAD]->(m:Geo)-[r:LINK*1..2]->(t:Geo) \
         WHERE size(r) >= 1 \
         RETURN s.id AS s, m.id AS m, t.id AS t ORDER BY s, m, t";
    let post = "MATCH (s:Geo {id:'s'})-[:ROAD]->(m:Geo)-[r:LINK*1..2]->(t:Geo) \
         WITH s, m, t, r WHERE size(r) >= 1 \
         RETURN s.id AS s, m.id AS m, t.id AS t ORDER BY s, m, t";
    take_early_evals();
    let rows = exec_cypher(&graph, early).rows;
    assert_eq!(
        take_early_evals(),
        0,
        "a rebinding name never evaluates early"
    );
    assert!(
        !rows.is_empty(),
        "the fixture has surviving paths: {rows:?}"
    );
    assert_eq!(
        rows,
        exec_cypher(&graph, post).rows,
        "declined vs post-filter"
    );
}

/// A null mid verdict (the property missing on one mid node) prunes
/// exactly: the post-filter drops the same rows.
#[test]
fn null_verdicts_prune_exactly() {
    let b = Builder::new();
    let s = b.node("s", &["Geo"]);
    let m1 = b.node("m1", &["Geo"]);
    let m2 = b.prop_node("m2", &["Geo"], &[("keep", Value::Bool(true))]);
    let t = b.node("t", &["Geo"]);
    b.rel(s, m1, "ROAD", &[]);
    b.rel(s, m2, "ROAD", &[]);
    b.rel(m1, t, "LINK", &[]);
    b.rel(m2, t, "LINK", &[]);
    let graph = b.done();
    let early = "MATCH (s:Geo {id:'s'})-[:ROAD]->(m:Geo)-[:LINK*1..1]->(t:Geo) \
         WHERE m.keep = true \
         RETURN s.id AS s, m.id AS m ORDER BY s, m";
    let post = "MATCH (s:Geo {id:'s'})-[:ROAD]->(m:Geo)-[:LINK*1..1]->(t:Geo) \
         WITH s, m WHERE m.keep = true \
         RETURN s.id AS s, m.id AS m ORDER BY s, m";
    let early_rows = exec_cypher(&graph, early).rows;
    assert_eq!(
        early_rows.len(),
        1,
        "the null mid node drops, the true one passes: {early_rows:?}"
    );
    assert_eq!(early_rows, exec_cypher(&graph, post).rows);
}

/// A raising mid predicate defers (the allReduce poison rule, walk-wide):
/// on a dead prefix — the raising mid node's tail has no complete path —
/// the post-filter never evaluates it, so no error may surface from the
/// early evaluation; on a surviving row both spellings raise identically.
#[test]
fn raising_conjuncts_defer_and_surface_only_where_the_postfilter_raises() {
    // Dead prefix: m1 (the % 0 operand) has no complete path, so the
    // post-filter never evaluates its predicate — the early evaluation
    // raises, defers, and the answers agree.
    let b = Builder::new();
    let s = b.node("s", &["Geo"]);
    let m1 = b.prop_node(
        "m1",
        &["Geo"],
        &[("keep", Value::Int(5)), ("zero", Value::Int(0))],
    );
    let m2 = b.prop_node(
        "m2",
        &["Geo"],
        &[("keep", Value::Int(5)), ("zero", Value::Int(1))],
    );
    let t = b.node("t", &["Geo"]);
    b.rel(s, m1, "ROAD", &[]);
    b.rel(s, m2, "ROAD", &[]);
    b.rel(m2, t, "LINK", &[]);
    let graph = b.done();
    let early = "MATCH (s:Geo {id:'s'})-[:ROAD]->(m:Geo)-[:LINK*1..1]->(t:Geo) \
         WHERE m.keep / m.zero = 1 \
         RETURN s.id AS s, m.id AS m ORDER BY s, m";
    let post = "MATCH (s:Geo {id:'s'})-[:ROAD]->(m:Geo)-[:LINK*1..1]->(t:Geo) \
         WITH s, m WHERE m.keep / m.zero = 1 \
         RETURN s.id AS s, m.id AS m ORDER BY s, m";
    let early_rows = try_exec_cypher(&graph, early);
    assert!(
        early_rows.is_ok(),
        "the raising dead prefix defers: {:?}",
        early_rows.err()
    );
    // The evaluation-disabled run is the exactness leg for errors (the
    // WITH-WHERE spelling drops an erroring row by pre-existing design —
    // `project.rs`'s `.ok()` retain — the allReduce suite's documented
    // exception): the deferred run must answer exactly the stood-down run.
    assert_eq!(
        early_rows.unwrap().rows,
        with_early_predicates_disabled(|| try_exec_cypher(&graph, early).unwrap()).rows,
        "deferred vs evaluation-disabled"
    );
    let post_rows = try_exec_cypher(&graph, post);
    assert!(
        post_rows.is_ok(),
        "the post-filter never sees the dead prefix"
    );

    // Surviving row: the only mid node's operand is zero — the row's own
    // post-filter raises, and the early evaluation defers to it: the same
    // error, the same spelling, as the evaluation-disabled run (the
    // WITH-WHERE spelling is not a leg — it drops the erroring row).
    let b = Builder::new();
    let s = b.node("s", &["Geo"]);
    let m = b.prop_node(
        "m",
        &["Geo"],
        &[("keep", Value::Int(5)), ("zero", Value::Int(0))],
    );
    let t = b.node("t", &["Geo"]);
    b.rel(s, m, "ROAD", &[]);
    b.rel(m, t, "LINK", &[]);
    let graph = b.done();
    let early_err = try_exec_cypher(&graph, early).err();
    let disabled_err = with_early_predicates_disabled(|| try_exec_cypher(&graph, early)).err();
    assert!(
        early_err.is_some(),
        "the surviving row raises through the post-filter"
    );
    assert_eq!(
        early_err.unwrap().to_string(),
        disabled_err.unwrap().to_string(),
        "the deferred error is the stood-down run's error"
    );
}

/// A path selector chooses among the pattern's FULL candidate set: the
/// MATCH-WHERE predicate narrows the already-selected path and can answer
/// nothing (CIP-60's selective path patterns, scenario 8) — so no conjunct
/// may evaluate early under a selector. The CIP-60 shape, pinned: the
/// blocked route is the longer one, the shortest one fails the predicate,
/// and the answer is empty.
#[test]
fn selector_patterns_never_evaluate_early() {
    let b = Builder::new();
    let s = b.prop_node("s", &["Acc"], &[("owner", Value::String("Scott".into()))]);
    let m = b.prop_node("m", &["Acc"], &[("owner", Value::String("Mike".into()))]);
    let ch = b.prop_node(
        "ch",
        &["Acc"],
        &[("owner", Value::String("Charles".into()))],
    );
    let j = b.prop_node(
        "j",
        &["Acc"],
        &[
            ("owner", Value::String("Jay".into())),
            ("blocked", Value::Bool(true)),
        ],
    );
    let k = b.prop_node("k", &["Acc"], &[("owner", Value::String("Kim".into()))]);
    b.rel(s, m, "T", &[]);
    b.rel(m, ch, "T", &[]);
    b.rel(s, j, "T", &[]);
    b.rel(j, k, "T", &[]);
    b.rel(k, ch, "T", &[]);
    let graph = b.done();
    let early = "MATCH p = ALL SHORTEST \
        (p1:Acc {owner: 'Scott'})-->+(q1:Acc)-->+(r1:Acc {owner: 'Charles'}) \
        WHERE q1.blocked = true \
        RETURN [n IN nodes(p) | n.owner] AS path";
    take_early_evals();
    let rows = exec_cypher(&graph, early).rows;
    assert_eq!(
        take_early_evals(),
        0,
        "a selector pattern never evaluates early"
    );
    assert!(
        rows.is_empty(),
        "the predicate narrows the already-selected shortest path to nothing: {rows:?}"
    );
}

/// The CIP-28 null pre-binding trap (ADR-0335, the hole the allReduce
/// round-1 review caught): a later-declared name referenced inside the
/// quantification is pre-bound to null on the walk's rows — the early
/// evaluation must not decide a conjunct over it on the pre-bound null
/// (readiness requires a non-null binding), only at the name's own
/// binding, so the answers match the post-filter's.
#[test]
fn cip28_null_prebindings_never_decide_early() {
    let b = Builder::new();
    let s = b.node("s", &["Geo"]);
    let m = b.node("m", &["Geo"]);
    let t = b.prop_node("t", &["Geo"], &[("q", Value::Int(2))]);
    b.rel(s, m, "LINK", &[]);
    b.rel(m, t, "LINK", &[]);
    let graph = b.done();
    // The group's inner node predicate references `t`, declared after the
    // quantification: inside it, `t` reads its null pre-binding (CIP-28
    // [5], the allReduce aliasing test's `b IS NULL` shape — a predicate
    // that passes on the pre-bound null, so paths survive to `t`'s
    // binding); the MATCH-level conjunct over `t` must evaluate only at
    // `t`'s own binding, never on the pre-bound null.
    let early = "MATCH (a:Geo {id:'s'}) ((y:Geo)-[r:LINK]-(x:Geo WHERE t.q IS NULL)){1,2} (t:Geo) \
         WHERE t.q > 1 \
         RETURN a.id AS a, t.id AS t ORDER BY a, t";
    let post = "MATCH (a:Geo {id:'s'}) ((y:Geo)-[r:LINK]-(x:Geo WHERE t.q IS NULL)){1,2} (t:Geo) \
         WITH a, t WHERE t.q > 1 \
         RETURN a.id AS a, t.id AS t ORDER BY a, t";
    take_early_evals();
    let early_rows = exec_cypher(&graph, early).rows;
    assert_eq!(
        early_rows.len(),
        1,
        "the surviving path names t: {early_rows:?}"
    );
    assert!(
        take_early_evals() >= 1,
        "the conjunct evaluated at t's binding"
    );
    assert_eq!(
        early_rows,
        exec_cypher(&graph, post).rows,
        "early vs post-filter under the pre-binding"
    );
}
