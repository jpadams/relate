//! Temporal truncation: the truncation-unit vocabulary, the date/time
//! truncation cascade, and the extras map's field overrides. Extracted from
//! `construct.rs` (ADR-0340).

use std::collections::BTreeMap;

use chrono::{
    Datelike, Duration as ChronoDur, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Weekday,
};

use crate::{Error, Result, Value};

use super::construct::{
    date_from_parts, date_from_week, date_of, local_dt_of, map_int, map_int_alias,
    time_from_map_with_base, time_of,
};
use super::field_rules::DAY_OF_WEEK_KEYS;
use super::zone::{offset_from_map, sub_offset, IanaAt, ResolvedZone};
use super::{
    nanos_to_time, now_utc_dt, range_err, time_to_nanos, ClockKind, Temporal, NANOS_PER_DAY,
    NANOS_PER_HOUR, NANOS_PER_MICRO, NANOS_PER_MILLI, NANOS_PER_MINUTE, NANOS_PER_SECOND,
};

/// The zone a truncation result keeps: an explicit `timezone` in the extras
/// map wins (resolved at `at`), otherwise the input value's own.
fn inherit_offset(value: &Temporal) -> ResolvedZone {
    match value {
        Temporal::DateTime { offset, zone, .. } => ResolvedZone {
            offset: *offset,
            zone: zone.clone(),
        },
        Temporal::Time { offset, .. } => ResolvedZone {
            offset: *offset,
            zone: None,
        },
        _ => ResolvedZone::utc(),
    }
}

/// Apply the extras map's date fields to a truncated date.
fn apply_date_extras(d: NaiveDate, m: &BTreeMap<String, Value>) -> Result<NaiveDate> {
    let mut out = d;
    if let Some(day) = map_int(m, "day")? {
        let day = u32::try_from(day).map_err(|_| Error::ty("invalid date"))?;
        out = date_from_parts(out.year(), out.month(), day)?;
    }
    if let Some(dow) = map_int_alias(m, DAY_OF_WEEK_KEYS)? {
        let current = i64::from(out.weekday().number_from_monday());
        let delta = dow.checked_sub(current).ok_or_else(range_err)?;
        let dur = ChronoDur::try_days(delta).ok_or_else(range_err)?;
        out = out.checked_add_signed(dur).ok_or_else(range_err)?;
    }
    Ok(out)
}

/// Truncation unit vocabulary, parsed once at the `truncate` entry so the
/// date/time helpers match on a typed unit instead of re-classifying names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TruncUnit {
    Millennium,
    Century,
    Decade,
    Year,
    WeekYear,
    Quarter,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
    Nanosecond,
}

impl TruncUnit {
    fn parse(unit: &str) -> Result<TruncUnit> {
        Ok(match unit {
            "millennium" => TruncUnit::Millennium,
            "century" => TruncUnit::Century,
            "decade" => TruncUnit::Decade,
            "year" => TruncUnit::Year,
            "weekyear" => TruncUnit::WeekYear,
            "quarter" => TruncUnit::Quarter,
            "month" => TruncUnit::Month,
            "week" => TruncUnit::Week,
            "day" => TruncUnit::Day,
            "hour" => TruncUnit::Hour,
            "minute" => TruncUnit::Minute,
            "second" => TruncUnit::Second,
            "millisecond" => TruncUnit::Millisecond,
            "microsecond" => TruncUnit::Microsecond,
            "nanosecond" => TruncUnit::Nanosecond,
            _ => return Err(Error::ty("unknown truncate unit")),
        })
    }

    /// The nanosecond length the unit rounds a time-of-day to. A
    /// date-granularity unit zeroes the whole time part, i.e. rounds to a
    /// day — the total table has no hole, and
    /// [`TruncUnit::is_date_granularity`] reads exactly that row.
    fn time_quantum(self) -> i64 {
        let nanos = match self {
            TruncUnit::Hour => NANOS_PER_HOUR,
            TruncUnit::Minute => NANOS_PER_MINUTE,
            TruncUnit::Second => NANOS_PER_SECOND,
            TruncUnit::Millisecond => NANOS_PER_MILLI,
            TruncUnit::Microsecond => NANOS_PER_MICRO,
            TruncUnit::Nanosecond => 1,
            TruncUnit::Millennium
            | TruncUnit::Century
            | TruncUnit::Decade
            | TruncUnit::Year
            | TruncUnit::WeekYear
            | TruncUnit::Quarter
            | TruncUnit::Month
            | TruncUnit::Week
            | TruncUnit::Day => NANOS_PER_DAY,
        };
        // The nanosecond constants fit i64 by construction.
        i64::try_from(nanos).expect("the nanosecond quanta fit i64")
    }

    /// Whether the unit names a date field (a year or coarser, down to the
    /// day) rather than a time field: the unit whose quantum is a whole day.
    fn is_date_granularity(self) -> bool {
        self.time_quantum() == day_quantum()
    }
}

/// A day's nanosecond length as [`TruncUnit::time_quantum`] spells it.
fn day_quantum() -> i64 {
    i64::try_from(NANOS_PER_DAY).expect("a day's nanoseconds fit i64")
}

/// `truncate(unit)` — the one-argument form truncates the current value of
/// `kind` (the transaction clock's reading, like an explicit `<kind>()`).
/// The form takes no fields map, so there is none to forward.
pub fn truncate_now(kind: ClockKind, unit: &str) -> Result<Temporal> {
    truncate(kind, unit, &super::clock::now_value(kind), None)
}

pub fn truncate(
    kind: ClockKind,
    unit: &str,
    value: &Temporal,
    map: Option<&Value>,
) -> Result<Temporal> {
    let extra = match map {
        Some(Value::Map(m)) => Some(m),
        Some(Value::Null) | None => None,
        _ => return Err(Error::ty("truncate map")),
    };
    // `Value::Map` shares its map behind an `Arc`; the helpers take the map.
    let extra = extra.map(|m| &**m);
    let unit = TruncUnit::parse(&unit.to_ascii_lowercase())?;
    check_unit(kind, unit, value)?;
    match kind {
        ClockKind::Date => {
            let d = date_of(value)?;
            let mut out = truncate_date(d, unit)?;
            if let Some(m) = extra {
                out = apply_date_extras(out, m)?;
            }
            Ok(Temporal::Date(out))
        }
        ClockKind::LocalDateTime => Ok(Temporal::LocalDateTime(truncate_to_local(
            value, unit, extra,
        )?)),
        ClockKind::DateTime => {
            let out = truncate_to_local(value, unit, extra)?;
            let resolved = resolve_offset(extra, value, IanaAt::Local(out))?;
            let utc = sub_offset(out, resolved.offset)?;
            Ok(Temporal::DateTime {
                utc,
                offset: resolved.offset,
                zone: resolved.zone,
            })
        }
        ClockKind::LocalTime => Ok(Temporal::LocalTime(truncated_time(value, unit, extra)?)),
        ClockKind::Time => {
            let tm = truncated_time(value, unit, extra)?;
            // A `time` truncate carries no instant of its own, so a
            // `timezone` in the extras map is resolved at the statement's
            // pinned UTC instant (`now_utc_dt`, the `now_*` family's
            // member) — and that instant is read as a UTC instant
            // (`IanaAt::Utc`), not as a UTC wall clock re-read in the zone.
            let resolved = resolve_offset(extra, value, IanaAt::Utc(now_utc_dt()))?;
            Ok(Temporal::Time {
                time: tm,
                offset: resolved.offset,
            })
        }
    }
}

/// Reject a unit the kind cannot truncate to: a date has no time field to
/// round, a time has no date field, and a datetime cannot use a time-based
/// unit on a date-only input. The reference engine reports all three as a
/// `TypeError`; the engine used to apply the quantum anyway.
fn check_unit(kind: ClockKind, unit: TruncUnit, value: &Temporal) -> Result<()> {
    // The capability partition stated once on the value type: a date-only
    // value carries a date and no time of day.
    let date_only = value.has_date() && !value.has_time();
    let rejected = match kind {
        ClockKind::Date => !unit.is_date_granularity(),
        ClockKind::LocalTime | ClockKind::Time => {
            unit.is_date_granularity() && unit != TruncUnit::Day
        }
        ClockKind::LocalDateTime | ClockKind::DateTime => date_only && !unit.is_date_granularity(),
    };
    if rejected {
        return Err(Error::ty(
            "truncation unit is not supported for this temporal kind",
        ));
    }
    Ok(())
}

/// Truncate a time-of-day value to `unit`, then apply the extras map's time
/// fields — the shared head of the `time` and `localtime` arms.
fn truncated_time(
    value: &Temporal,
    unit: TruncUnit,
    extra: Option<&BTreeMap<String, Value>>,
) -> Result<NaiveTime> {
    let tm = time_of(value).ok_or_else(|| Error::ty("truncate time"))?;
    let mut tm = truncate_time(tm, unit);
    if let Some(m) = extra {
        tm = time_from_map_with_base(tm, m)?;
    }
    Ok(tm)
}

/// Truncate `value`'s local wall clock to `unit`, then apply the extras
/// map's date and time fields to the truncated result.
fn truncate_to_local(
    value: &Temporal,
    unit: TruncUnit,
    extra: Option<&BTreeMap<String, Value>>,
) -> Result<NaiveDateTime> {
    let dt = local_dt_of(value)?;
    let mut out = truncate_date(dt.date(), unit)?.and_time(truncate_time(dt.time(), unit));
    if let Some(m) = extra {
        out = apply_date_extras(out.date(), m)?.and_time(time_from_map_with_base(out.time(), m)?);
    }
    Ok(out)
}

/// The zone a truncation result keeps: an explicit `timezone` in the extras
/// map wins (resolved at `at`), otherwise the input value's own.
fn resolve_offset(
    extra: Option<&BTreeMap<String, Value>>,
    value: &Temporal,
    at: IanaAt,
) -> Result<ResolvedZone> {
    match extra.filter(|m| m.contains_key("timezone")) {
        Some(m) => offset_from_map(m, at),
        None => Ok(inherit_offset(value)),
    }
}

fn truncate_date(d: NaiveDate, unit: TruncUnit) -> Result<NaiveDate> {
    Ok(match unit {
        TruncUnit::Millennium => date_from_parts(d.year() / 1000 * 1000, 1, 1)?,
        TruncUnit::Century => date_from_parts(d.year() / 100 * 100, 1, 1)?,
        TruncUnit::Decade => date_from_parts(d.year() / 10 * 10, 1, 1)?,
        TruncUnit::Year => date_from_parts(d.year(), 1, 1)?,
        TruncUnit::WeekYear => {
            let iso = d.iso_week();
            date_from_week(iso.year(), 1, Weekday::Mon)?
        }
        TruncUnit::Quarter => {
            let q = (d.month() - 1) / 3 * 3 + 1;
            date_from_parts(d.year(), q, 1)?
        }
        TruncUnit::Month => date_from_parts(d.year(), d.month(), 1)?,
        TruncUnit::Week => {
            // `NaiveDate - TimeDelta` panics when the Monday of this week
            // falls before `NaiveDate::MIN`; use the checked operator.
            let back = i64::from(d.weekday().num_days_from_monday());
            d.checked_sub_signed(ChronoDur::days(back))
                .ok_or_else(range_err)?
        }
        // Day and every time-granularity unit leave the date untouched.
        _ => d,
    })
}

fn truncate_time(t: NaiveTime, unit: TruncUnit) -> NaiveTime {
    // A date-granularity unit zeroes the whole time part, i.e. rounds to a
    // day — the quantum the total table answers for those units.
    let q = unit.time_quantum();
    let frac = t.nanosecond();
    // chrono encodes a leap second as second 59 with a fraction >= 1e9, i.e.
    // past the end of the day on the nanosecond timeline, where the quantum
    // formula below would wrap it to midnight. Truncate the represented
    // second instead, dropping the leap marker for second-or-coarser units.
    if frac >= 1_000_000_000 {
        let secs = t.num_seconds_from_midnight();
        let (secs, frac) = if q >= 1_000_000_000 {
            let q_secs = (q / 1_000_000_000) as u32;
            (secs - secs % q_secs, 0)
        } else {
            let sub = frac - 1_000_000_000;
            (secs, 1_000_000_000 + (sub - sub.rem_euclid(q as u32)))
        };
        return NaiveTime::from_num_seconds_from_midnight_opt(secs, frac).unwrap_or(NaiveTime::MIN);
    }
    let n = time_to_nanos(t);
    nanos_to_time(n - n.rem_euclid(q))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    /// A unit's granularity is per kind: a date cannot round a time field, a
    /// time cannot round a date field, and a datetime refuses a time-based
    /// unit when the input carries no time.
    #[test]
    fn truncation_unit_granularity_is_per_kind() {
        let date = Temporal::Date(NaiveDate::from_ymd_opt(1984, 2, 11).unwrap());
        assert!(truncate(ClockKind::Date, "hour", &date, None).is_err());
        assert!(truncate(ClockKind::Date, "microsecond", &date, None).is_err());
        assert!(truncate(ClockKind::Date, "day", &date, None).is_ok());

        let time = Temporal::LocalTime(NaiveTime::from_hms_opt(12, 30, 0).unwrap());
        assert!(truncate(ClockKind::LocalTime, "year", &time, None).is_err());
        assert!(truncate(ClockKind::LocalTime, "week", &time, None).is_err());
        assert!(truncate(ClockKind::LocalTime, "day", &time, None).is_ok());
        assert!(truncate(ClockKind::LocalTime, "hour", &time, None).is_ok());

        assert!(truncate(ClockKind::DateTime, "minute", &date, None).is_err());
        assert!(truncate(ClockKind::LocalDateTime, "hour", &date, None).is_err());
        assert!(truncate(ClockKind::LocalDateTime, "day", &date, None).is_ok());
    }
}
