//! The component-field map policy's unit suite: the date-form classifier, the
//! prerequisite and cascade rules, the source-field vocabulary, and the two
//! epoch spellings' exclusions.

use super::clock::ClockKind;
use super::field_rules::*;
use super::Temporal;
use crate::{Error, Result, Value};

fn map(entries: &[(&str, Value)]) -> Fields {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

fn class<T>(result: Result<T>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(Error::ArgumentError(_)) => "ArgumentError",
        Err(Error::Type(_)) => "TypeError",
        Err(other) => panic!("unexpected error class: {other:?}"),
    }
}

fn date(entries: &[(&str, Value)]) -> &'static str {
    class(validate(ClockKind::Date, &map(entries)))
}

fn localdatetime(entries: &[(&str, Value)]) -> &'static str {
    class(validate(ClockKind::LocalDateTime, &map(entries)))
}

fn datetime(entries: &[(&str, Value)]) -> &'static str {
    class(validate(ClockKind::DateTime, &map(entries)))
}

fn localtime(entries: &[(&str, Value)]) -> &'static str {
    class(validate(ClockKind::LocalTime, &map(entries)))
}

/// The four date forms are mutually exclusive (22G05), while each form's
/// own spelling is legal.
#[test]
fn date_forms_are_exclusive() {
    for entries in [
        &[
            ("year", Value::Int(1984)),
            ("month", Value::Int(2)),
            ("week", Value::Int(11)),
        ][..],
        &[
            ("year", Value::Int(1984)),
            ("month", Value::Int(2)),
            ("dayOfWeek", Value::Int(6)),
        ],
        &[
            ("year", Value::Int(1984)),
            ("ordinalDay", Value::Int(173)),
            ("day", Value::Int(2)),
        ],
        &[
            ("year", Value::Int(1984)),
            ("quarter", Value::Int(2)),
            ("day", Value::Int(11)),
        ],
        &[
            ("year", Value::Int(1984)),
            ("week", Value::Int(2)),
            ("quarter", Value::Int(11)),
        ],
    ] {
        assert_eq!(date(entries), "TypeError", "{entries:?}");
    }
    for entries in [
        &[
            ("year", Value::Int(1984)),
            ("month", Value::Int(2)),
            ("day", Value::Int(11)),
        ][..],
        &[("year", Value::Int(1984)), ("ordinalDay", Value::Int(173))],
        &[
            ("year", Value::Int(1984)),
            ("week", Value::Int(2)),
            ("dayOfWeek", Value::Int(3)),
        ],
        &[
            ("year", Value::Int(1984)),
            ("quarter", Value::Int(2)),
            ("dayOfQuarter", Value::Int(10)),
        ],
    ] {
        assert_eq!(date(entries), "ok", "{entries:?}");
    }
}

/// The classifier answers one form per field set — the answer the reader
/// consumes, so a null-valued field is absent for both.
#[test]
fn the_classifier_names_the_selected_form() {
    let form = |entries: &[(&str, Value)]| {
        validate(ClockKind::Date, &map(entries))
            .expect("valid date map")
            .form
    };
    assert_eq!(
        form(&[("year", Value::Int(1984)), ("month", Value::Int(2))]),
        Some(DateForm::Calendar)
    );
    assert_eq!(
        form(&[
            ("year", Value::Int(1984)),
            ("week", Value::Int(2)),
            ("dayOfWeek", Value::Int(3)),
        ]),
        Some(DateForm::Week)
    );
    assert_eq!(
        form(&[("year", Value::Int(1984)), ("ordinalDay", Value::Int(173))]),
        Some(DateForm::Ordinal)
    );
    assert_eq!(
        form(&[
            ("year", Value::Int(1984)),
            ("quarter", Value::Int(2)),
            ("dayOfQuarter", Value::Int(10)),
        ]),
        Some(DateForm::Quarter)
    );
    // `year` alone is the default calendar form.
    assert_eq!(
        form(&[("year", Value::Int(1984))]),
        Some(DateForm::Calendar)
    );
    // A null-valued form field is absent, not a selection: the answer is
    // the source-supplied date, so no form shifts it.
    let source = Value::Temporal(Temporal::Date(
        chrono::NaiveDate::from_ymd_opt(2021, 1, 1).unwrap(),
    ));
    assert_eq!(
        form(&[("date", source), ("week", Value::Null)]),
        None,
        "a null `week` must not select the week form"
    );
}

/// A component named without its prerequisite is 22G05, except a missing
/// `year`, which is the 22007 family.
#[test]
fn component_prerequisites_are_typed() {
    assert_eq!(
        date(&[("year", Value::Int(1984)), ("day", Value::Int(11))]),
        "TypeError"
    );
    assert_eq!(
        date(&[("year", Value::Int(1984)), ("dayOfWeek", Value::Int(3))]),
        "TypeError"
    );
    assert_eq!(
        date(&[("year", Value::Int(1984)), ("dayOfQuarter", Value::Int(3))]),
        "TypeError"
    );
    assert_eq!(date(&[("month", Value::Int(2))]), "ArgumentError");
    assert_eq!(date(&[("week", Value::Int(2))]), "ArgumentError");
}

/// A time field makes the date form complete: the default calendar form
/// needs month and day, the other forms their own companion.
#[test]
fn a_time_field_requires_a_complete_date() {
    assert_eq!(
        localdatetime(&[("year", Value::Int(1984)), ("hour", Value::Int(11))]),
        "TypeError"
    );
    assert_eq!(
        localdatetime(&[
            ("year", Value::Int(1984)),
            ("month", Value::Int(2)),
            ("hour", Value::Int(11)),
        ]),
        "TypeError"
    );
    assert_eq!(
        localdatetime(&[
            ("year", Value::Int(1984)),
            ("week", Value::Int(2)),
            ("hour", Value::Int(11)),
        ]),
        "TypeError"
    );
    assert_eq!(
        datetime(&[
            ("year", Value::Int(1984)),
            ("quarter", Value::Int(2)),
            ("hour", Value::Int(11)),
        ]),
        "TypeError"
    );
    assert_eq!(
        localdatetime(&[
            ("year", Value::Int(1984)),
            ("month", Value::Int(2)),
            ("day", Value::Int(11)),
            ("hour", Value::Int(11)),
        ]),
        "ok"
    );
    // A `date` source supplies the missing components.
    let source = Value::Temporal(Temporal::Date(
        chrono::NaiveDate::from_ymd_opt(1984, 2, 11).unwrap(),
    ));
    assert_eq!(
        datetime(&[("date", source), ("hour", Value::Int(11))]),
        "ok"
    );
}

/// The time cascade: hour before minute before second before subsecond.
#[test]
fn time_fields_need_the_coarser_component() {
    assert_eq!(localtime(&[("minute", Value::Int(30))]), "TypeError");
    assert_eq!(
        localtime(&[("hour", Value::Int(12)), ("second", Value::Int(45))]),
        "TypeError"
    );
    assert_eq!(
        localtime(&[("hour", Value::Int(12)), ("minute", Value::Int(31))]),
        "ok"
    );
    assert_eq!(
        localtime(&[("hour", Value::Int(12)), ("millisecond", Value::Int(645))]),
        "TypeError"
    );
    assert_eq!(
        localtime(&[
            ("hour", Value::Int(12)),
            ("minute", Value::Int(31)),
            ("nanosecond", Value::Int(1)),
        ]),
        "TypeError"
    );
}

/// Unknown or kind-incompatible keys are the 22G05 `Not supported`
/// family, even with a null value; an empty map is 22007.
#[test]
fn vocabulary_and_emptiness() {
    assert_eq!(
        date(&[("year", Value::Int(1984)), ("foo", Value::Int(1))]),
        "TypeError"
    );
    assert_eq!(date(&[("time", Value::Null)]), "TypeError");
    assert_eq!(localtime(&[("year", Value::Null)]), "TypeError");
    assert_eq!(date(&[]), "ArgumentError");
    assert_eq!(localtime(&[]), "ArgumentError");
}

/// A time zone beside other fields is rejected on the zoneless kinds and
/// legal on the zoned ones.
#[test]
fn timezone_is_a_clock_reading_on_zoneless_kinds() {
    assert_eq!(
        date(&[
            ("year", Value::Int(1984)),
            ("month", Value::Int(2)),
            ("day", Value::Int(11)),
            ("timezone", Value::String("+01:00".into())),
        ]),
        "TypeError"
    );
    assert_eq!(
        localdatetime(&[
            ("year", Value::Int(1984)),
            ("month", Value::Int(2)),
            ("day", Value::Int(11)),
            ("timezone", Value::String("+01:00".into())),
        ]),
        "TypeError"
    );
    assert_eq!(
        datetime(&[
            ("year", Value::Int(1984)),
            ("month", Value::Int(2)),
            ("day", Value::Int(11)),
            ("timezone", Value::String("+01:00".into())),
        ]),
        "ok"
    );
}

/// The source fields must carry the part they select, and `datetime`
/// excludes `date`/`time`. A present non-temporal source value is the
/// same typed rejection — never a silently absent field.
#[test]
fn source_fields_select_a_part() {
    let date_value = Value::Temporal(Temporal::Date(
        chrono::NaiveDate::from_ymd_opt(1984, 2, 11).unwrap(),
    ));
    let time_value = Value::Temporal(Temporal::LocalTime(
        chrono::NaiveTime::from_hms_opt(12, 30, 0).unwrap(),
    ));
    assert_eq!(localtime(&[("time", date_value.clone())]), "TypeError");
    assert_eq!(date(&[("date", time_value.clone())]), "TypeError");
    assert_eq!(
        localdatetime(&[
            ("datetime", date_value.clone()),
            ("date", date_value.clone()),
        ]),
        "ArgumentError"
    );
    assert_eq!(localdatetime(&[("datetime", date_value)]), "TypeError");
    assert_eq!(localdatetime(&[("time", time_value)]), "ArgumentError");
    // A non-temporal source value is a typed rejection, not an absent
    // field the constructor defaults over.
    assert_eq!(date(&[("date", Value::Int(42))]), "TypeError");
    assert_eq!(localdatetime(&[("time", Value::Int(42))]), "TypeError");
    assert_eq!(
        datetime(&[
            ("year", Value::Int(2020)),
            ("month", Value::Int(1)),
            ("day", Value::Int(1)),
            ("time", Value::Int(42)),
        ]),
        "TypeError"
    );
}

/// The day-of-week value range.
#[test]
fn day_of_week_is_one_to_seven() {
    for day in [0, -1, 8] {
        assert_eq!(
            date(&[
                ("year", Value::Int(1984)),
                ("week", Value::Int(2)),
                ("dayOfWeek", Value::Int(day)),
            ]),
            "ArgumentError",
            "dayOfWeek {day}"
        );
    }
}

/// The epoch form's exclusions are 22007, and only `datetime` accepts
/// the form at all. Both spellings of each field are the same field, so
/// the exclusion and the selection see them alike.
#[test]
fn epoch_form_exclusions() {
    let epoch =
        |entries: &[(&str, Value)]| class(validate_epoch(ClockKind::DateTime, &map(entries)));
    assert_eq!(
        epoch(&[
            ("epochSeconds", Value::Int(1)),
            ("epochMillis", Value::Int(2)),
        ]),
        "ArgumentError"
    );
    // The lowercase spelling is the same field: mixing it with the
    // camelCase sibling of the other field is the same exclusion.
    assert_eq!(
        epoch(&[
            ("epochSeconds", Value::Int(1)),
            ("epochmillis", Value::Int(2)),
        ]),
        "ArgumentError"
    );
    let at = Value::Temporal(Temporal::DateTime {
        utc: chrono::NaiveDate::from_ymd_opt(2025, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        offset: 0,
        zone: Some("Z".into()),
    });
    assert_eq!(
        epoch(&[("epochSeconds", Value::Int(1)), ("datetime", at)]),
        "ArgumentError"
    );
    assert_eq!(
        class(validate_epoch(
            ClockKind::Date,
            &map(&[("epochSeconds", Value::Int(1))])
        )),
        "TypeError"
    );
}

/// Every accepted epoch spelling selects its own field, so the reader
/// reads exactly the field the validator selected.
#[test]
fn epoch_selection_names_the_present_spelling() {
    let select = |entries: &[(&str, Value)]| {
        validate_epoch(ClockKind::DateTime, &map(entries)).expect("valid epoch map")
    };
    assert_eq!(
        select(&[("epochseconds", Value::Int(1))]),
        EpochField::Seconds
    );
    assert_eq!(
        select(&[("epochMillis", Value::Int(1))]),
        EpochField::Millis
    );
    assert_eq!(EpochField::Seconds.split(5), (5, 0));
    assert_eq!(EpochField::Millis.split(1_234), (1, 234_000_000));
}
