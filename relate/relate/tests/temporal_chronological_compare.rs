//! The Cypher 25 chronological comparison operators
//! (`IS [NOT] BEFORE | AFTER | SAME INSTANT | SAME REPRESENTATION`) and the
//! Cypher 25 underscore digit separators, end to end through the engine —
//! the acceptance-matrix rows of the temporal-literals cluster
//! (TemporalChronologicalCompare / LiteralAcceptance).

mod common;

use relate::Value;

use common::{connection, row, scalar_one};

/// The four operators live behind the ADR-0371 `chronological-comparisons`
/// preview gate: `Database::open` resolves `RELATE_PREVIEW` once per
/// process, so this crate opts in once before its first open (the
/// `hybrid_search.rs` shape) and every test in it runs gated-on.
fn gated_connection() -> relate::Connection {
    common::set_preview_once("chronological-comparisons");
    connection()
}

/// The operator matrix: one query per row of the zoned-datetime and
/// zoned-time Examples tables, asserting all four operator answers.
#[test]
fn zoned_datetime_operator_matrix() {
    let mut c = gated_connection();
    for (a, b, si, sr, bef, aft) in [
        // (a, b, same instant, same representation, before, after)
        (
            "datetime('2020-01-01T12:00+01:00')",
            "datetime('2020-01-01T11:00Z')",
            true,
            false,
            false,
            false,
        ),
        (
            "datetime('2020-01-01T12:00+01:00')",
            "datetime('2020-01-01T12:00[Europe/Stockholm]')",
            true,
            false,
            false,
            false,
        ),
        (
            "datetime('2020-10-25T02:30+02:00[Europe/Stockholm]')",
            "datetime('2020-10-25T02:30+01:00[Europe/Stockholm]')",
            false,
            true,
            true,
            false,
        ),
        (
            "datetime('2020-01-01T12:00Z')",
            "datetime('2020-01-01T12:00+00:00')",
            true,
            true,
            false,
            false,
        ),
        (
            "datetime('1969-12-31T23:59:59Z')",
            "datetime('1970-01-01T00:59:59+01:00')",
            true,
            false,
            false,
            false,
        ),
    ] {
        let q = format!(
            "WITH {a} AS a, {b} AS b \
             RETURN a IS SAME INSTANT b AS sameInstant, \
                    a IS SAME REPRESENTATION b AS sameRepresentation, \
                    a IS BEFORE b AS before, \
                    a IS AFTER b AS after"
        );
        let vals = row(&mut c, &q);
        assert_eq!(
            vals,
            vec![
                Value::Bool(si),
                Value::Bool(sr),
                Value::Bool(bef),
                Value::Bool(aft)
            ],
            "{a} vs {b}"
        );
    }
}

/// Zoned times order by their UTC instant, unwrapped at midnight.
#[test]
fn zoned_time_instant_ordering() {
    let mut c = gated_connection();
    for (a, b, si, sr, bef, aft) in [
        (
            "time('02:30+02:00')",
            "time('00:30Z')",
            true,
            false,
            false,
            false,
        ),
        (
            "time('00:30+02:00')",
            "time('22:30Z')",
            false,
            false,
            true,
            false,
        ),
        (
            "time('23:30-02:00')",
            "time('01:30Z')",
            false,
            false,
            false,
            true,
        ),
        (
            "time('12:00Z')",
            "time('12:00+00:00')",
            true,
            true,
            false,
            false,
        ),
        (
            "time('12:00+02:00')",
            "time('12:00+01:00')",
            false,
            false,
            true,
            false,
        ),
    ] {
        let q = format!(
            "WITH {a} AS a, {b} AS b \
             RETURN a IS SAME INSTANT b AS sameInstant, \
                    a IS SAME REPRESENTATION b AS sameRepresentation, \
                    a IS BEFORE b AS before, \
                    a IS AFTER b AS after"
        );
        let vals = row(&mut c, &q);
        assert_eq!(
            vals,
            vec![
                Value::Bool(si),
                Value::Bool(sr),
                Value::Bool(bef),
                Value::Bool(aft)
            ],
            "{a} vs {b}"
        );
    }
}

/// Non-zoned temporals reduce to the ordinary comparison relations.
#[test]
fn non_zoned_temporals_reduce_to_ordinary_comparison() {
    let mut c = gated_connection();
    let vals = row(
        &mut c,
        "WITH date('2020-01-01') AS a, date('2020-01-02') AS b \
         RETURN a IS SAME INSTANT b AS sameInstant, \
                a IS SAME REPRESENTATION b AS sameRepresentation, \
                a IS BEFORE b AS before, \
                a IS AFTER b AS after",
    );
    assert_eq!(
        vals,
        vec![
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false)
        ]
    );
    let vals = row(
        &mut c,
        "WITH localdatetime('2020-01-01T10:00') AS a, localdatetime('2020-01-01T10:00') AS b \
         RETURN a IS SAME INSTANT b AS sameInstant, \
                a IS SAME REPRESENTATION b AS sameRepresentation, \
                a IS BEFORE b AS before, \
                a IS AFTER b AS after",
    );
    assert_eq!(
        vals,
        vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false)
        ]
    );
}

/// Null operands stay null in every form, NOT included.
#[test]
fn null_operands_stay_null() {
    let mut c = gated_connection();
    let vals = row(
        &mut c,
        "RETURN null IS BEFORE date('2020-01-01') AS b1, \
                date('2020-01-01') IS BEFORE null AS b2, \
                null IS NOT BEFORE date('2020-01-01') AS b3, \
                null IS SAME INSTANT date('2020-01-01') AS b4, \
                null IS AFTER null AS b8",
    );
    assert_eq!(vals, vec![Value::Null; 5]);
}

/// Non-temporal and mixed-kind operands: ordering null, SAME false —
/// and the NOT forms flip exactly the false ones.
#[test]
fn non_temporal_and_mixed_operands() {
    let mut c = gated_connection();
    let vals = row(
        &mut c,
        "RETURN 1 IS BEFORE 2 AS beforeNonTemporal, \
                'x' IS AFTER time('12:00Z') AS afterNonTemporal, \
                time('12:00Z') IS SAME INSTANT 'x' AS sameInstantNonTemporal, \
                date('2020-01-01') IS BEFORE datetime('2020-01-01T00:00Z') AS beforeMixed, \
                1 IS NOT SAME REPRESENTATION 1 AS notSameRepNonTemporal, \
                1 IS NOT BEFORE 2 AS notBeforeNonTemporal",
    );
    assert_eq!(
        vals,
        vec![
            Value::Null,
            Value::Null,
            Value::Bool(false),
            Value::Null,
            Value::Bool(true),
            Value::Null,
        ]
    );
}

/// The operator words stay usable as plain identifiers.
#[test]
fn operator_words_stay_non_reserved_identifiers() {
    let mut c = gated_connection();
    assert_eq!(
        scalar_one(
            &mut c,
            "WITH 1 AS before, 2 AS after, 3 AS same, 4 AS instant, 5 AS representation \
             RETURN before + after + same + instant + representation AS total"
        ),
        Value::Int(15)
    );
}

/// The simple-CASE prefix form applies the predicate to the operand.
#[test]
fn case_prefix_form() {
    let mut c = gated_connection();
    assert_eq!(
        scalar_one(
            &mut c,
            "RETURN CASE datetime('2020-01-01T00:00Z') \
                     WHEN IS BEFORE datetime('2020-01-02T00:00Z') THEN 'before' \
                     WHEN IS NOT SAME INSTANT datetime('2020-01-01T00:00Z') THEN 'different' \
                     ELSE 'same' END AS result"
        ),
        Value::String("before".into())
    );
}

/// The operator composes with AND/OR/NOT at the usual precedences.
#[test]
fn precedence_with_not_and_or() {
    let mut c = gated_connection();
    let vals = row(
        &mut c,
        "RETURN date('2020-01-01') IS BEFORE date('2020-01-02') AND \
                date('2020-01-03') IS AFTER date('2020-01-02') AS conjunction, \
                NOT date('2020-01-02') IS BEFORE date('2020-01-01') AND \
                (date('2020-01-01') IS SAME INSTANT date('2020-01-01') OR \
                 date('2020-01-01') IS SAME REPRESENTATION date('2020-01-02')) AS mixed",
    );
    assert_eq!(vals, vec![Value::Bool(true), Value::Bool(true)]);
}

/// The predicate filters rows from a property, scan-shaped.
#[test]
fn property_predicate_in_where() {
    let mut c = gated_connection();
    for q in [
        "CREATE (:Event {at: datetime('2020-01-01T10:00Z')})",
        "CREATE (:Event {at: datetime('2020-01-01T12:00+01:00')})",
        "CREATE (:Event {at: datetime('2020-01-01T11:00Z')})",
    ] {
        row(&mut c, &format!("{q} RETURN 1 AS ok"));
    }
    assert_eq!(
        scalar_one(
            &mut c,
            "MATCH (n:Event) WHERE n.at IS SAME INSTANT datetime('2020-01-01T11:00Z') \
             RETURN count(n) AS matches"
        ),
        Value::Int(2)
    );
    assert_eq!(
        scalar_one(
            &mut c,
            "MATCH (n:Event) WHERE n.at IS BEFORE datetime('2020-01-01T11:00Z') \
             RETURN count(n) AS matches"
        ),
        Value::Int(1)
    );
}

/// The Cypher 25 underscore digit separators, through the engine.
#[test]
fn underscore_digit_separators() {
    let mut c = gated_connection();
    for (q, want) in [
        ("RETURN 4_2 AS l", Value::Int(42)),
        ("RETURN -4_2 AS l", Value::Int(-42)),
        ("RETURN 1_0_0_0 AS l", Value::Int(1000)),
        ("RETURN 0x_2A_40_7F AS l", Value::Int(2769023)),
        ("RETURN -0x_2A_40_7F AS l", Value::Int(-2769023)),
        ("RETURN 0o_66_44_22 AS l", Value::Int(223506)),
        ("RETURN -0o_66_44_22 AS l", Value::Int(-223506)),
    ] {
        assert_eq!(scalar_one(&mut c, q), want, "{q}");
    }
    assert_eq!(
        scalar_one(&mut c, "RETURN 1_000.000_1 AS l"),
        Value::Float(1000.0001)
    );
}
