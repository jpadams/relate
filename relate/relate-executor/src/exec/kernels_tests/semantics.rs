//! The kernels' semantics suites: existence against the probe,
//! three-valued null predicate logic, and error parity on a bad bound
//! operand.

use relate_core::Value;

use super::super::kernels::{take_kernel_engagements, with_kernels_disabled};
use super::super::test_support::{error_of, try_exec_cypher};
use super::{differential, families};

/// Existence: the boolean-count fold's `count(DISTINCT var.key) > 0` —
/// the early-exit road to both kernels.
#[test]
fn existence_matches_the_probe() {
    let g = families();
    let has_pair = differential(
        &g,
        "MATCH (h:Hub)-[:A]->(t:Tag), (h)-[:B]->(u:Tag) WHERE t.name = 'x' \
         RETURN count(DISTINCT h.k) > 0 AS has_pair",
        true,
    );
    assert_eq!(has_pair.rows, vec![vec![Value::Bool(true)]]);

    // A false answer runs the whole hub population.
    let no_pair = differential(
        &g,
        "MATCH (h:Hub)-[:A]->(t:Tag), (h)-[:B]->(u:Tag) WHERE t.name = 'zz' \
         RETURN count(DISTINCT h.k) > 0 AS has_pair",
        true,
    );
    assert_eq!(no_pair.rows, vec![vec![Value::Bool(false)]]);

    // The shared-terminal cycle and the chain family's existence.
    differential(
        &g,
        "MATCH (h:Hub)-[:C]->(m:Mid)-[:D]->(t:Tag), (h)-[:A]->(t) \
         RETURN count(DISTINCT h.k) > 0 AS has_cycle",
        true,
    );
    differential(
        &g,
        "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) WHERE g.name = 'two' \
         RETURN count(DISTINCT a.k) > 0 AS has_a",
        true,
    );
}

/// Null semantics: a missing property is a null slot (IS NULL keeps it,
/// comparisons drop it), and a null bound drops every row.
#[test]
fn null_predicates_keep_three_valued_logic() {
    let g = families();
    let is_null = differential(
        &g,
        "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) WHERE a.s IS NULL \
         RETURN a.k AS ak LIMIT 99",
        true,
    );
    // a2 carries no `s`.
    assert_eq!(is_null.rows, vec![vec![Value::Int(2)]]);
    let not_null = differential(
        &g,
        "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) WHERE a.s IS NOT NULL \
         RETURN a.k AS ak LIMIT 99",
        true,
    );
    assert_eq!(not_null.rows.len(), 4);
    let null_bound = differential(
        &g,
        "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) WHERE a.k > null \
         RETURN a.k AS ak LIMIT 99",
        true,
    );
    assert_eq!(null_bound.rows, Vec::<Vec<Value>>::new());
}

/// Error parity: an operand only the shared `binary` rejects at evaluation
/// time errors on both paths with the same message, and a population the
/// admission never reaches never evaluates it at all.
#[test]
fn error_parity_on_a_bad_bound_operand() {
    let g = families();
    let cypher = "MATCH (a:Alpha)-[:R]->(b:Beta)-[:S]->(g:Gamma) WHERE g.name =~ '[' \
                  RETURN a.k AS ak LIMIT 99";
    let kernaled = error_of(try_exec_cypher(&g, cypher));
    // The engagement take is also the drain: the errored run's count must
    // not outlive this test — and it pins that the kernel path ran at
    // all (a gate that stopped engaging this shape would silently turn
    // both sides into the per-row path and the parity assert vacuous).
    assert_eq!(
        take_kernel_engagements(),
        1,
        "the kernel path takes {cypher}"
    );
    let per_row = with_kernels_disabled(|| error_of(try_exec_cypher(&g, cypher)));
    assert_eq!(kernaled, per_row);
    // A label the store never populated: no candidate reaches the
    // predicate, so neither path errors.
    differential(
        &g,
        "MATCH (a:Nope)-[:R]->(b:Beta)-[:S]->(g:Gamma) WHERE g.name =~ '[' \
         RETURN a.k AS ak LIMIT 99",
        true,
    );
}
