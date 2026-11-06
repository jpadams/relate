//! The chain family's differential suite: corridor walks, the
//! rel-uniqueness rule, multi-chunk widths, and the anonymous and
//! degenerate shapes — every query the chain kernel takes answers
//! exactly what the per-row path answers.

use std::collections::BTreeMap;

use relate_core::Value;

use super::super::kernels::{take_kernel_engagements, with_kernels_disabled};
use super::super::test_support::{exec_autocommit, try_exec_cypher_params, Builder};
use super::{differential, families};

/// The chain family: the corridor walk with predicates at both ends, a
/// mid predicate, undirected hops, a named rel, and a parameter bound.
#[test]
fn chain_matches_the_row_path() {
    let g = families();
    let both_ends = differential(
        &g,
        "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) \
         WHERE a.k >= 1 AND g.name CONTAINS 'o' RETURN a.k AS ak, g.name AS gn LIMIT 99",
        true,
    );
    // a1 -> b1 -> {one, two}, a2 -> b2 -> two, a3 -> b1 -> {one, two}:
    // five trails.
    assert_eq!(both_ends.rows.len(), 5);
    assert_eq!(both_ends.columns, vec!["ak".to_string(), "gn".to_string()]);

    // A mid-node predicate, undirected hops, and a named rel.
    let undirected = differential(
        &g,
        "MATCH (a:Alpha)-[r:R]-(b:Beta)-[:S]-(g:Gamma) WHERE b.k > 15 \
         RETURN a.k AS ak, g.name AS gn LIMIT 99",
        true,
    );
    assert_eq!(
        undirected.rows,
        vec![vec![Value::Int(2), Value::String("two".into())]]
    );

    // A parameter bound evaluates once per clause, not per row.
    let params = BTreeMap::from([("min".to_string(), Value::Int(2))]);
    let kernaled = try_exec_cypher_params(
        &g,
        "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) WHERE a.k >= $min \
         RETURN a.k AS ak LIMIT 99",
        &params,
    )
    .unwrap();
    assert_eq!(take_kernel_engagements(), 1);
    let per_row = with_kernels_disabled(|| {
        try_exec_cypher_params(
            &g,
            "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) WHERE a.k >= $min \
             RETURN a.k AS ak LIMIT 99",
            &params,
        )
    })
    .unwrap();
    assert_eq!(kernaled.rows, per_row.rows);
}

/// The chain's rel-uniqueness rule: two hops over the same type cannot
/// take the same rel — parallel rels widen the trail set, and an
/// undirected pair over one rel cannot serve both hops of one trail.
#[test]
fn chain_keeps_the_different_relationships_rule() {
    let g = Builder::new().done();
    exec_autocommit(
        &g,
        "CREATE (x1:X)-[:K]->(y1:Y), (x1)-[:K]->(y1), (y1)-[:K]->(z1:Z), (y1)-[:K]->(z2:Z)",
    )
    .unwrap();
    // 2 first-hop choices x 2 second-hop rels = 4 trails.
    let trails = differential(
        &g,
        "MATCH (x:X)-[:K]->(y:Y)-[:K]->(z:Z) RETURN x.k AS xk, z.k AS zk LIMIT 99",
        true,
    );
    assert_eq!(trails.rows.len(), 4);
    // Two rels between one pair: both orderings of the undirected pair
    // are trails (each hop takes its own rel).
    exec_autocommit(&g, "CREATE (p1:P)-[:Q]->(m1:M), (m1)-[:Q]->(p1)").unwrap();
    let both_ways = differential(
        &g,
        "MATCH (p:P)-[:Q]-(m:M)-[:Q]-(w:P) RETURN p.k AS pk LIMIT 99",
        true,
    );
    assert_eq!(both_ways.rows.len(), 2);
    // One rel between the pair: no trail can take it twice — zero rows.
    exec_autocommit(&g, "CREATE (s1:S)-[:Q]->(n1:N)").unwrap();
    let one_rel = differential(
        &g,
        "MATCH (s:S)-[:Q]-(n)-[:Q]-(w) RETURN s.k AS sk LIMIT 99",
        true,
    );
    assert_eq!(one_rel.rows, Vec::<Vec<Value>>::new());
}

/// A population past `CHUNK_ROWS` (512) through the **chain kernel**: a
/// 3-hop corridor whose stages batch past one chunk boundary, engagement
/// counted — the batch chain's own chunk case (`batch_tests`) owns the
/// scan shape; this one pins the contracted kernel's width handling.
#[test]
fn the_chain_kernel_batches_past_one_chunk() {
    let g = Builder::new().done();
    let mut create = String::from("CREATE ");
    for i in 0..600 {
        if i > 0 {
            create.push_str(", ");
        }
        create.push_str(&format!("(:Bulk {{n: {i}}})"));
    }
    exec_autocommit(&g, &create).unwrap();
    let mut next = String::from("CREATE ");
    for i in 0..100 {
        if i > 0 {
            next.push_str(", ");
        }
        next.push_str(&format!(
            "(:Tip {{t: {i}}})-[:PREV]->(:Bulk {{n: {i}}})-[:TAIL]->(:End)"
        ));
    }
    exec_autocommit(&g, &next).unwrap();
    let trails = differential(
        &g,
        "MATCH (t:Tip)-[:PREV]->(b:Bulk)-[:TAIL]->(e:End) RETURN b.n AS bn LIMIT 999",
        true,
    );
    assert_eq!(trails.rows.len(), 100);
}

/// The stellar pre-push suspects (blockers 9–11): the anonymous-binding
/// shapes and the one-node side. Every verdict is a differential — the
/// kernel path's rows exactly the per-row path's, engagement counted.
///
/// - **B11, rows (real bug, fixed):** a pattern that names no element
///   binds nothing, so the clause's output is one empty row per complete
///   binding — the row count comes from the walk, never a column's length
///   (a chunk with zero columns carries zero rows by construction, the
///   hole the suspect named; the labeled fully-anonymous chain reached it
///   and answered zero rows). Partially anonymous shapes project the
///   named remainder. A fully anonymous chain with no labels at all keeps
///   the walker: its start is unmeasurable, so the cost gate's rank
///   sentinel declines it.
/// - **B10, sides (real bug, declined):** a one-node pattern (zero hops)
///   reached the cost gate, whose terminal estimate read an empty side —
///   a reachable expect-panic. The matcher now declines the shape before
///   engagement.
/// - **B11, pair:** a fully anonymous pair shares no hub variable — the
///   hub-star is defined by one — so it declines; the hub's own variable
///   keeps the hub-star's projection non-empty by construction, and its
///   zero-binding rows mode emits nothing off the binding counter.
#[test]
fn anonymous_and_degenerate_shapes_match_the_row_path() {
    let g = families();
    // The fully anonymous chain with labels: the corridor's five trails,
    // each an empty binding — the shape that answered zero rows before
    // the breaker's count moved off the columns.
    let anonymous = differential(
        &g,
        "MATCH (:Alpha)-[:R]->(:Beta)-[:S]->(:Gamma) RETURN 1 AS one LIMIT 99",
        true,
    );
    assert_eq!(anonymous.rows.len(), 5);
    assert_eq!(anonymous.rows[0], vec![Value::Int(1)]);
    // The same shape with no labels: unmeasurable start, the cost gate's
    // rank sentinel declines it — the walker's rows identical.
    differential(
        &g,
        "MATCH ()-[:R]->()-[:S]->() RETURN 1 AS one LIMIT 99",
        false,
    );
    // Partially anonymous: an anonymous mid, anonymous far ends — the
    // named remainder is the projection. The far-ends shape keeps the
    // walker: the planner leaves its unmeasurable anonymous end at the
    // scan end, so the cost gate declines it — rows identical either way.
    differential(
        &g,
        "MATCH (a:Alpha)-[:R]->()-[:S]->() RETURN a.k AS ak LIMIT 99",
        true,
    );
    differential(
        &g,
        "MATCH ()-[:R]->(b:Beta)-[:S]->() RETURN b.k AS bk LIMIT 99",
        false,
    );
    differential(
        &g,
        "MATCH (a:Alpha)-[:R]->(b)-[:S]->(g:Gamma) RETURN a.k AS ak LIMIT 99",
        true,
    );
    differential(
        &g,
        "MATCH (h:Hub)-[:A]->(), (h)-[:B]->(t:Tag) RETURN h.k AS hk, t.name AS tn LIMIT 99",
        true,
    );
    // Existence over the fully anonymous chain: every complete binding
    // counts, none of them projects.
    let exists = differential(
        &g,
        "RETURN EXISTS { MATCH (:Alpha)-[:R]->(:Beta)-[:S]->(:Gamma) } AS hit",
        true,
    );
    assert_eq!(exists.rows, vec![vec![Value::Bool(true)]]);

    // A one-node pattern is not a side: declined at the matcher (0
    // engagements), the walker's rows identical — both orderings, and the
    // both-sides-one-node shape.
    differential(
        &g,
        "MATCH (h:Hub), (h)-[:A]->(t:Tag) RETURN h.k AS hk, t.name AS tn LIMIT 99",
        false,
    );
    differential(
        &g,
        "MATCH (h:Hub)-[:A]->(t:Tag), (h) RETURN h.k AS hk, t.name AS tn LIMIT 99",
        false,
    );
    differential(
        &g,
        "MATCH (h:Hub {k: 5}), (h) RETURN h.k AS hk LIMIT 99",
        false,
    );
    // A fully anonymous pair shares no hub variable: no hub-star.
    differential(
        &g,
        "MATCH ()-[:A]->(), ()-[:B]->() RETURN 1 AS one LIMIT 99",
        false,
    );
    // The hub-star's zero-binding rows mode: nothing emitted, off the
    // binding counter rather than a column's emptiness.
    let none = differential(
        &g,
        "MATCH (h:Hub)-[:A]->(t:Tag), (h)-[:B]->(u:Tag) WHERE t.name = 'zz' \
         RETURN h.k AS hk LIMIT 99",
        true,
    );
    assert_eq!(none.rows, Vec::<Vec<Value>>::new());
}
