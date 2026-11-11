//! ADR-0576 Decision 2: the statement preamble's `CYPHER <version>`
//! clause is removed — every statement runs the engine's one dialect, so
//! a statement opening with a `CYPHER` prefix fails with one authored
//! typed error, before any other processing, while `EXPLAIN`/`PROFILE`
//! keep their preamble role and still produce plans.

mod common;

use common::{connection, err_text};

/// The authored removal text (ADR-0576 Decision 2, normative, verbatim).
const PREFIX_ERROR_TEXT: &str = "the CYPHER version prefix is no longer supported. Every statement runs the latest Cypher; please remove the prefix and resend the statement";

#[test]
fn every_cypher_version_prefix_fails_with_the_authored_error() {
    for src in [
        "CYPHER 5 RETURN 1 AS n",
        "CYPHER 25 RETURN 1 AS n",
        "CYPHER 4 RETURN 1 AS n",
        // The whole preamble clause is gone: a version-less options
        // clause, and a prefix riding after EXPLAIN, fail the same way.
        "CYPHER runtime=slotted RETURN 1 AS n",
        "EXPLAIN CYPHER 5 RETURN 1 AS n",
    ] {
        let err = err_text(connection().query(src));
        assert!(
            err.contains(PREFIX_ERROR_TEXT),
            "wrong outcome for {src:?}: {err:?}"
        );
    }
}

#[test]
fn explain_and_profile_still_produce_plans() {
    for src in ["EXPLAIN RETURN 1 AS n", "PROFILE RETURN 1 AS n"] {
        let result = connection()
            .query(src)
            .unwrap_or_else(|e| panic!("{src:?}: {e}"));
        assert!(result.plan.is_some(), "{src:?}: no plan");
    }
}

#[test]
fn the_when_query_repro_fails_prefixed_and_runs_unprefixed() {
    let err = err_text(connection().query("CYPHER 5 WHEN true THEN RETURN 2 AS x"));
    assert!(
        err.contains(PREFIX_ERROR_TEXT),
        "the prefixed repro must fail with the authored error: {err:?}"
    );
    let result = connection().query("WHEN true THEN RETURN 2 AS x").unwrap();
    assert_eq!(result.rows, vec![vec![relate::Value::Int(2)]]);
}
