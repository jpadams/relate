//! Regression pins for the syntax-gap round's engine regressions that need
//! a preview opt-in, so they run in their own test binary (ADR-0371: the
//! preview set is process-global, read once per `Database::open`).
//!
//! GpmSyntaxMixingAllowedAcceptance "Mixing : conjunction with GPM-only
//! label/type expression operators" (42I10): the
//! `mixed-label-syntax` opt-in admits mixing ACROSS the label expressions
//! of one clause, but a colon-conjunction combined with a GQL symbol inside
//! ONE label expression stays illegal — exactly as Neo4j's allowed-suite
//! pins it.

mod common;

use common::{connection, err_text, set_preview_once};

const PREVIEW: &str = "mixed-label-syntax";

#[test]
fn within_one_label_expression_the_mixing_stays_rejected() {
    set_preview_once(PREVIEW);
    let mut c = connection();
    for query in [
        "MATCH (n:A|B:C) RETURN n",
        "CREATE (n:A:B&C)",
        "MERGE (n:A:B&C)",
    ] {
        let error = err_text(c.query(query));
        assert!(
            error.contains("InvalidLabelExpression"),
            "{query}\n  expected the within-expression mixing rejection, got: {error}"
        );
    }
}

#[test]
fn mixing_across_separate_label_expressions_is_admitted() {
    set_preview_once(PREVIEW);
    let mut c = connection();
    // The corpus's own positive shape: a colon chain on one element beside a
    // GQL disjunction of a conjunction on another.
    c.query("MATCH (n:A:B:C)-->(m:(A&B)|C) RETURN n")
        .expect("separate label expressions may mix under the opt-in");
    c.query("CREATE (n:A:B {name: 'x'})")
        .expect("a pure colon-conjunction CREATE stays legal");
    c.query("RETURN 1 AS ok").unwrap();
}
