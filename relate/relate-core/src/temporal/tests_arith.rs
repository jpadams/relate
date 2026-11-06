//! Temporal arithmetic tests: date-plus-duration, scaling and the civil-day
//! pair, beside the functions they cover (the cross-module temporal behavior
//! stays in `tests.rs`, the `duration.between` family in `tests_between.rs`,
//! and the out-of-range sweep in `tests_ranges.rs`). Split from the inline
//! `mod tests` in `arith.rs`.

use std::collections::BTreeMap;

use super::*;
use crate::temporal::{construct_date, construct_duration, parse_iso};
use crate::Value;

/// A `construct_duration` argument map from `(field, value)` pairs — the one
/// spelling of the integer-field fixtures these tests share.
fn dur_map(fields: &[(&str, i64)]) -> Value {
    Value::Map(
        fields
            .iter()
            .map(|(key, value)| ((*key).into(), Value::Int(*value)))
            .collect::<BTreeMap<_, _>>()
            .into(),
    )
}

#[test]
fn date_plus_fractional_duration() {
    let date = construct_date(Some(&Value::Map(
        [
            ("year".into(), Value::Int(1984)),
            ("month".into(), Value::Int(10)),
            ("day".into(), Value::Int(11)),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>()
        .into(),
    )))
    .unwrap();
    let dur = construct_duration(Some(&Value::Map(
        [
            ("years".into(), Value::Float(12.5)),
            ("months".into(), Value::Float(5.5)),
            ("days".into(), Value::Float(14.5)),
            ("hours".into(), Value::Float(16.5)),
            ("minutes".into(), Value::Float(12.5)),
            ("seconds".into(), Value::Float(70.5)),
            ("nanoseconds".into(), Value::Int(3)),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>()
        .into(),
    )))
    .unwrap();
    assert_eq!(
        add_duration(&date, &dur).unwrap().iso_string(),
        "1997-10-11"
    );
    assert_eq!(
        sub_duration(&date, &dur).unwrap().iso_string(),
        "1971-10-12"
    );
}

#[test]
fn date_plus_duration_months_and_neg_days() {
    let d = construct_duration(Some(&dur_map(&[
        ("months", 1),
        ("days", -14),
        ("hours", 16),
        ("minutes", -12),
        ("seconds", 70),
    ])))
    .unwrap();
    let date = construct_date(Some(&Value::Map(
        [
            ("year".into(), Value::Int(1984)),
            ("month".into(), Value::Int(10)),
            ("day".into(), Value::Int(11)),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>()
        .into(),
    )))
    .unwrap();
    let sum = add_duration(&date, &d).unwrap();
    let diff = sub_duration(&date, &d).unwrap();
    assert_eq!(sum.iso_string(), "1984-10-28", "sum {sum:?}");
    assert_eq!(diff.iso_string(), "1984-09-25", "diff {diff:?}");
}

#[test]
fn scale_duration_half() {
    let d = construct_duration(Some(&dur_map(&[
        ("years", 12),
        ("months", 5),
        ("days", 14),
        ("hours", 16),
        ("minutes", 12),
        ("seconds", 70),
        ("nanoseconds", 1),
    ])))
    .unwrap();
    let half = scale_duration(&d, 0.5).unwrap();
    assert_eq!(half.iso_string(), "P6Y2M22DT13H21M8S");
}

/// A scaled duration whose seconds field leaves the `i64` range is
/// Neo4j's interval-field overflow (22N28), not a silently larger
/// duration: `duration({seconds: i64::MAX}) * 2.0` stays inside the
/// `i128` nanosecond field but doubles an unrepresentable seconds field.
#[test]
fn scaling_past_the_i64_seconds_field_is_an_error() {
    let max = construct_duration(Some(&dur_map(&[("seconds", i64::MAX)]))).unwrap();
    // The unscaled and identity-scaled forms are representable.
    assert!(scale_duration(&max, 1.0).is_ok());
    assert_eq!(
        scale_duration(&max, 2.0),
        Err(Error::ty("temporal value out of range"))
    );
    // The same bound applies to duration + duration.
    assert_eq!(
        add_duration(&max, &max),
        Err(Error::ty("temporal value out of range"))
    );
}

/// A duration whose month field fits `i64` but whose average length in
/// seconds does not is an overflow (Neo4j's second bound) at every producer:
/// the map constructor, the ISO parser and scaling all reject the same
/// value. Verified against Neo4j 5 (`duration({months: 4000000000000})` →
/// "Invalid value for duration, will cause overflow"; `duration('P4000000000000M')`
/// and `duration({months: 4000000000000}) * 1.0` fail identically).
#[test]
fn the_average_length_bound_applies_to_every_producer() {
    // 4e12 months fits `i64` but is ~1.05e19 average seconds.
    let months = 4_000_000_000_000i64;
    assert_eq!(
        construct_duration(Some(&dur_map(&[("months", months)]))),
        Err(Error::ty("temporal value out of range"))
    );
    assert_eq!(
        parse_iso("duration", "P4000000000000M"),
        Err(Error::ty("temporal value out of range"))
    );
    let value = Temporal::Duration {
        months,
        days: 0,
        nanos: 0,
    };
    assert_eq!(
        scale_duration(&value, 1.0),
        Err(Error::ty("temporal value out of range"))
    );
    assert_eq!(
        add_duration(&value, &value),
        Err(Error::ty("temporal value out of range"))
    );
}

/// ADR-0325: the one civil-day count, from the proleptic Gregorian epoch
/// origin — civil-date arithmetic and the Bolt Date struct both read it.
#[test]
fn civil_epoch_days_counts_from_the_epoch() {
    assert_eq!(civil_epoch_days(1970, 1, 1), 0);
    assert_eq!(civil_epoch_days(2024, 1, 2), 19724);
    assert_eq!(civil_epoch_days(1969, 12, 31), -1);
}

/// The inverse holds over leap-day edges and both eras: every round trip
/// returns the input triple, including the Julian-era dates the proleptic
/// calendar spans.
#[test]
fn civil_from_days_inverts_civil_epoch_days() {
    for &(y, m, d) in &[
        (1970i64, 1u32, 1u32),
        (2024, 1, 2),
        (1969, 12, 31),
        (2000, 2, 29),
        (1900, 2, 28),
        (2100, 3, 1),
        (1, 1, 1),
        (-1, 12, 31),
    ] {
        let days = civil_epoch_days(y, m, d);
        assert_eq!(civil_from_days(days), (y, m, d));
    }
    assert_eq!(civil_from_days(0), (1970, 1, 1));
}

/// Negating a duration at the inclusive bound can step one second past it
/// (`duration_checked` floors a negative nanosecond remainder), so the
/// negation goes through the same assembly seam and reports the
/// interval-field overflow instead of building an out-of-bound value.
#[test]
fn negating_a_bound_edge_duration_is_the_interval_field_overflow() {
    let edge_nanos = field::MAX_DURATION_SECONDS * 1_000_000_000 + 1;
    // The positive value is inside the inclusive bound ...
    assert!(Temporal::duration_checked(0, 0, edge_nanos).is_ok());
    let edge = Temporal::Duration {
        months: 0,
        days: 0,
        nanos: edge_nanos,
    };
    // ... while its negation is one second past it.
    assert!(negate_duration(&edge).is_err());
    // A representable negation stays accepted.
    let small = Temporal::Duration {
        months: 0,
        days: 0,
        nanos: 1_000_000_000,
    };
    assert!(negate_duration(&small).is_ok());
}
