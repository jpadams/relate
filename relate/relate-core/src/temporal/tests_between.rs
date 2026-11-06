//! Temporal `duration.between` tests: the pair-domain classification and the
//! per-unit shaping, beside the functions they cover (the additive/scaling
//! family stays in `tests_arith.rs`, the cross-module temporal behavior in
//! `tests.rs`, and the out-of-range sweep in `tests_ranges.rs`).

use super::*;
use crate::temporal::parse_iso;

/// A `Temporal::Time` (offset-carrying time of day at UTC) fixture: the
/// time-only domain's shared operand shape.
fn time_of_day(hour: u32, minute: u32) -> Temporal {
    Temporal::Time {
        time: NaiveTime::from_hms_opt(hour, minute, 0).unwrap(),
        offset: 0,
    }
}

/// The zoned-datetime path delegates to the exported resolver, keeping its
/// `Option` contract: a zoned wall clock resolves, a DST gap yields `None`
/// and surfaces as the path's existing error (ADR-0178).
#[test]
fn duration_between_zoned_wall_clock_delegates_to_resolver() {
    let zoned = parse_iso("datetime", "2015-01-01T00:00[Europe/Stockholm]").unwrap();
    let local = Temporal::LocalDateTime(
        NaiveDate::from_ymd_opt(2015, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
    );
    assert_eq!(
        duration_between(&local, &zoned).unwrap().iso_string(),
        "PT0S"
    );

    let gap = Temporal::LocalDateTime(
        NaiveDate::from_ymd_opt(2021, 3, 28)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap(),
    );
    let gap_zone = parse_iso("datetime", "2021-03-28T01:00[Europe/Stockholm]").unwrap();
    assert_eq!(
        duration_between(&gap, &gap_zone),
        Err(Error::ty("duration.between"))
    );
}

/// The time-of-day domain has no months or days between operands, but a
/// readable date-like operand still yields the pinned all-zero triple through
/// `shaped_duration` (the guard that used to short-circuit this was removed
/// with its `mixed` flag).
#[test]
fn time_against_date_shapes_zero_months_and_days() {
    let time = time_of_day(12, 0);
    let date = Temporal::Date(NaiveDate::from_ymd_opt(2020, 1, 1).unwrap());
    assert_eq!(
        duration_between_unit(&time, &date, BetweenUnit::Months)
            .unwrap()
            .iso_string(),
        "PT0S"
    );
    assert_eq!(
        duration_between_unit(&time, &date, BetweenUnit::Days)
            .unwrap()
            .iso_string(),
        "PT0S"
    );
    assert_eq!(
        duration_between_unit(&date, &time, BetweenUnit::Full)
            .unwrap()
            .iso_string(),
        "PT12H"
    );
}

/// An unreadable operand in the time-of-day domain is the same typed
/// `duration.between` error for every unit: the deleted `mixed` guard used to
/// return a zero duration for months/days while full/seconds already errored.
#[test]
fn time_domain_unreadable_operand_errors_for_every_unit() {
    let time = time_of_day(12, 0);
    let duration = Temporal::Duration {
        months: 0,
        days: 0,
        nanos: 1,
    };
    for unit in [
        BetweenUnit::Full,
        BetweenUnit::Months,
        BetweenUnit::Days,
        BetweenUnit::Seconds,
    ] {
        assert_eq!(
            duration_between_unit(&time, &duration, unit),
            Err(Error::ty("duration.between"))
        );
    }
}

/// A context datetime whose wall clock leaves chrono's range has no reading
/// in the zone domain: the missing date propagates as the typed
/// `duration.between` error instead of anchoring the time of day to a
/// fabricated 1970-01-01.
#[test]
fn time_of_day_with_unreadable_context_errors() {
    let ctx = Temporal::DateTime {
        utc: NaiveDate::MAX.and_time(NaiveTime::MIN),
        // One day past chrono's maximum date.
        offset: 86_400,
        zone: Some("Europe/Stockholm".into()),
    };
    let local = Temporal::LocalTime(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    let zoned = time_of_day(12, 0);
    assert_eq!(
        duration_between(&local, &ctx),
        Err(Error::ty("duration.between"))
    );
    assert_eq!(
        duration_between(&zoned, &ctx),
        Err(Error::ty("duration.between"))
    );
}
