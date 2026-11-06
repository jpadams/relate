//! Two composition rules this crate owns are pinned here. The
//! `CALL … IN TRANSACTIONS` rule reads the one per-clause write answer
//! ([`crate::clause_mutates_store`]) — catalog writes included: a catalog
//! change persists while moving no ADR-0309 counter, and it is still a write
//! to every rule that rejects a write. The `USE` rule is positional, not
//! write-dependent (a `USE` after any preceding clause is misplaced). The
//! third write-predicate consumer, the expression-subquery rejection in
//! `expr_check`, is pinned end to end in the facade's
//! `relate/tests/namespace.rs`.

use relate_front::parse_and_lower;
use relate_ir::{Clause, Expr};

use crate::binding::Env;
use crate::validate::validate_part_with_env;

/// Lower `cypher` and run the semantic clause validation on its one part,
/// returning the rejection's text.
fn rejection(cypher: &str) -> String {
    let query = parse_and_lower(cypher).unwrap_or_else(|error| panic!("{cypher}: {error}"));
    let [part] = query.parts.as_slice() else {
        panic!("{cypher}: expected one part");
    };
    validate_part_with_env(part, super::Scope::stage(Env::new()), &mut |_| false)
        .expect_err("the composition rule rejects this statement")
        .to_string()
}

/// The `USE` placement rule is positional: any clause ahead of it — write or
/// read — makes it misplaced. (An earlier revision asked the write predicate
/// here; the rule no longer reads write history, so the test pins the
/// positional answer with a non-writing preceding clause.)
///
/// The `USE` clause is inserted by hand because the front-end's own placement
/// gate refuses a misplaced `USE` during lowering — for every statement the
/// product admits, that gate answers first. This planner arm is the backstop
/// for a plan that reached the planner some other way.
#[test]
fn use_after_a_preceding_clause_is_rejected() {
    let mut query = parse_and_lower("MATCH (n) RETURN n").expect("the statement lowers");
    let [part] = query.parts.as_mut_slice() else {
        panic!("expected one part");
    };
    part.clauses.insert(
        1,
        Clause::Use {
            graph: Expr::String("neo4j".to_string()),
        },
    );
    let error = validate_part_with_env(part, super::Scope::stage(Env::new()), &mut |_| false)
        .expect_err("the placement rule rejects a USE after another clause")
        .to_string();
    assert!(
        error.contains("InvalidPlacementOfUseClause: USE must be the first clause"),
        "unexpected error: {error}"
    );
}

/// A catalog write cannot precede `CALL { … } IN TRANSACTIONS`; the rule asks
/// "did a write run before the batched subquery?", and a catalog write did.
#[test]
fn catalog_write_before_call_in_transactions_is_rejected() {
    let error = rejection(
        "ALTER CURRENT GRAPH TYPE SET GRAPH TYPE SOGraph \
         CALL { CREATE (n:Boxed) } IN TRANSACTIONS OF 1 ROWS",
    );
    assert!(
        error.contains("WriteBeforeCallInTransactions"),
        "unexpected error: {error}"
    );
}
