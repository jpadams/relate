//! The `chronological-comparisons` preview gate, CLOSED side (ADR-0371):
//! without the opt-in every Cypher 25 chronological operator fails closed
//! with the authored disabled-feature error — the form
//! `TemporalChronologicalCompareDisabled`'s scenario pins.
//!
//! This file is its own test binary: `RELATE_PREVIEW` is process-global
//! and read once per `Database::open` (ADR-0371), so it must never share a
//! process with the enabled side (`chronological_comparisons_gate.rs`).

mod common;

use common::{assert_preview_closed, connection};
use relate_core::PreviewFeature;

#[test]
fn every_operator_needs_the_opt_in() {
    for query in [
        "RETURN date('2020-01-01') IS BEFORE date('2020-01-02') AS r",
        "RETURN date('2020-01-02') IS AFTER date('2020-01-01') AS r",
        "RETURN date('2020-01-01') IS SAME INSTANT date('2020-01-01') AS r",
        "RETURN date('2020-01-01') IS SAME REPRESENTATION date('2020-01-01') AS r",
        "RETURN date('2020-01-01') IS NOT BEFORE date('2020-01-02') AS r",
    ] {
        assert_preview_closed(PreviewFeature::ChronologicalComparisons, query);
    }
}

#[test]
fn the_gate_reaches_nested_spellings() {
    // The detector walks the whole statement tree, so a gated operator in a
    // CASE WHEN or a subquery body fails closed the same way.
    assert_preview_closed(
        PreviewFeature::ChronologicalComparisons,
        "RETURN CASE date('2020-01-01') WHEN IS BEFORE date('2020-01-02') \
         THEN 'before' ELSE 'not' END AS result",
    );
    assert_preview_closed(
        PreviewFeature::ChronologicalComparisons,
        "MATCH (n) WHERE date('2020-01-01') IS BEFORE date('2020-01-02') RETURN n",
    );
}

#[test]
fn ordinary_comparisons_stay_open() {
    let mut c = connection();
    c.query("RETURN 1 < 2 AS r")
        .expect("an ordinary comparison is not gated");
}
