//! The constructor field-map policy: which keys each clock kind accepts,
//! which combinations are legal, and which prerequisites a partial field set
//! must satisfy.
//!
//! The reference engine rejects a key the kind cannot consume (`Not
//! supported: hour` on a date), two date forms mixed (`Cannot assign week to
//! calendar date`), a component without its prerequisite (`day cannot be
//! specified without month`), and a time zone beside other fields on a
//! zoneless kind. [`validate`] is the one statement of that policy, consulted
//! once per component-field map before construction; the constructors
//! themselves then read only fields this policy admitted — and the date form
//! and source temporals it resolved, so presence is decided once rather than
//! re-inferred by the reader.

use std::collections::BTreeMap;

use crate::{Error, Result, Value};

use super::clock::ClockKind;
use super::Temporal;

/// The component-field map type: the constructor maps, the epoch validator
/// and the readers all spell this one map.
pub(super) type Fields = BTreeMap<String, Value>;

/// The multi-spelling aliases of a component field, shared by every probe
/// and read so the camelCase/lowercase pairs are never spelled twice. The
/// canonical spelling comes first in each family.
pub(super) const DAY_OF_WEEK_KEYS: &[&str] = &["dayOfWeek", "dayofweek"];
pub(super) const ORDINAL_DAY_KEYS: &[&str] = &["ordinalDay", "ordinalday"];
pub(super) const DAY_OF_QUARTER_KEYS: &[&str] = &["dayOfQuarter", "dayofquarter"];
/// The epoch field spellings, shared by the form classifier, the exclusion
/// policy and the reader so the three cannot disagree about what names an
/// epoch.
pub(super) const EPOCH_MILLIS_KEYS: &[&str] = &["epochMillis", "epochmillis"];
pub(super) const EPOCH_SECONDS_KEYS: &[&str] = &["epochSeconds", "epochseconds"];

/// The one presence notion: a field counts when it holds a non-null value. A
/// null field is ignored by every reader, so it neither satisfies nor
/// violates a rule, and the form classifier below decides presence the same
/// way the reader does.
fn present_value<'a>(m: &'a Fields, key: &str) -> Option<&'a Value> {
    m.get(key).filter(|value| !value.is_null())
}

/// Whether `key` holds a non-null value (see [`present_value`]). The one
/// presence notion the readers consume too, so the validator and a reader
/// cannot decide a field's presence differently.
pub(super) fn present(m: &Fields, key: &str) -> bool {
    present_value(m, key).is_some()
}

/// Whether any spelling of an aliased field carries a non-null value — the
/// presence notion the form classifier and the epoch router both read, so a
/// null-valued spelling neither selects a form nor routes a map to the epoch
/// path.
pub(super) fn present_alias(m: &Fields, keys: &[&str]) -> bool {
    keys.iter().any(|key| present(m, key))
}

/// The date-component spellings, derived from the one form table: `year`
/// plus every row's primary and day keys. A new form row is therefore
/// accepted as a date field without a second vocabulary to edit.
fn is_date_field(key: &str) -> bool {
    key == "year"
        || DATE_FORM_RULES
            .iter()
            .any(|rule| rule.primary_keys.contains(&key) || rule.day_keys.contains(&key))
}

/// The time-component spellings, shared by every kind that carries a time.
fn is_time_field(key: &str) -> bool {
    matches!(
        key,
        "hour" | "minute" | "second" | "millisecond" | "microsecond" | "nanosecond"
    )
}

/// The epoch-component spellings, derived from the shared families. Only
/// `datetime` consumes them ([`ClockKind::has_epoch`]); the epoch router
/// reads the same predicate, so the form classifier and the reader cannot
/// disagree about what names an epoch.
pub(super) fn is_epoch_field(key: &str) -> bool {
    EPOCH_MILLIS_KEYS.contains(&key) || EPOCH_SECONDS_KEYS.contains(&key)
}

/// Whether `kind` consumes the map key `key`. The one key vocabulary: an
/// accepted key is defined here, and the readers below read exactly these.
/// Every rule reads a [`ClockKind`] capability predicate, so the kind's parts
/// are stated once.
fn accepts(kind: ClockKind, key: &str) -> bool {
    match key {
        // A time zone is vocabulary for every kind; whether it may appear
        // beside other fields is [`check_timezone`]'s rule.
        "timezone" => true,
        "date" => kind.has_date(),
        "time" => kind.has_time(),
        "datetime" => kind.has_date_and_time(),
        _ => {
            is_date_field(key) && kind.has_date()
                || is_time_field(key) && kind.has_time()
                || is_epoch_field(key) && kind.has_epoch()
        }
    }
}

/// The date form a component map's own fields select. Classified once in
/// [`check_date_fields`] and returned by [`validate`], so the reader
/// (`construct::date_from_map`) consumes the same presence decision instead
/// of re-inferring it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DateForm {
    /// `year` + `month` + `day` (the default form).
    Calendar,
    /// `year` + `week` + `dayOfWeek`.
    Week,
    /// `year` + `ordinalDay`.
    Ordinal,
    /// `year` + `quarter` + `dayOfQuarter`.
    Quarter,
}

/// One date form's fields: the primary selector, the day selector beside it,
/// and whether a time-bearing map needs both. The one table the four forms'
/// key sets, prerequisite rules and rejection spellings come from.
struct DateFormRule {
    form: DateForm,
    /// The form's primary keys, canonical spelling first (`month`, `week`,
    /// `ordinalDay`, `quarter`).
    primary_keys: &'static [&'static str],
    /// The form's day keys, canonical spelling first; empty when `year` plus
    /// the primary already completes the date (the ordinal form).
    day_keys: &'static [&'static str],
    /// Whether a `time` field makes the primary and day fields mandatory.
    time_needs_both: bool,
}

/// The four form rows. The calendar row is first: it is the default when a
/// map names a `year` and no form field ([`check_date_fields`] reads it by
/// position, never by a second lookup).
const DATE_FORM_RULES: &[DateFormRule] = &[
    DateFormRule {
        form: DateForm::Calendar,
        primary_keys: &["month"],
        day_keys: &["day"],
        time_needs_both: true,
    },
    DateFormRule {
        form: DateForm::Week,
        primary_keys: &["week"],
        day_keys: DAY_OF_WEEK_KEYS,
        time_needs_both: true,
    },
    DateFormRule {
        form: DateForm::Ordinal,
        primary_keys: ORDINAL_DAY_KEYS,
        day_keys: &[],
        time_needs_both: false,
    },
    DateFormRule {
        form: DateForm::Quarter,
        primary_keys: &["quarter"],
        day_keys: DAY_OF_QUARTER_KEYS,
        time_needs_both: true,
    },
];

impl DateFormRule {
    /// Whether the map's own fields select this form.
    fn selects(&self, m: &Fields) -> bool {
        present_alias(m, self.primary_keys) || present_alias(m, self.day_keys)
    }

    /// The form's canonical primary spelling (`week`), for messages.
    fn primary_name(&self) -> &'static str {
        self.primary_keys
            .first()
            .copied()
            .expect("every date form names a primary field")
    }

    /// The form's canonical day spelling (`dayOfWeek`), for messages.
    fn day_name(&self) -> Option<&'static str> {
        self.day_keys.first().copied()
    }
}

/// The one date-form classifier: the single rule the map's fields select, or
/// `None` when it names no form. Naming two forms at once is the exclusion
/// rejection. The rule itself is answered (not just its form), so the
/// caller reads the row the classifier already found.
fn date_form(m: &Fields) -> Result<Option<&'static DateFormRule>> {
    let mut selected = DATE_FORM_RULES.iter().filter(|rule| rule.selects(m));
    let first = selected.next();
    if selected.next().is_some() {
        return Err(Error::ty(
            "Cannot combine calendar, week, ordinal and quarter date fields.",
        ));
    }
    Ok(first)
}

/// The required-companion failure: a component named without the field its
/// form needs. The reference engine reports these under 22G05
/// (`TypeError`), the same family as the mutually exclusive date forms.
fn required(field: &str) -> Error {
    Error::ty(format!("{field} must be specified"))
}

/// A missing `year` is the one prerequisite the reference engine reports as
/// an `ArgumentError` (22007), not as 22G05.
fn required_year() -> Error {
    Error::argument_error("year must be specified")
}

/// The one emptiness spelling: a map with no non-null field, or an epoch map
/// with no epoch value, names no temporal unit. The duration map constructor
/// shares it, so one policy has one text.
pub(super) fn at_least_one_unit() -> Error {
    Error::argument_error("At least one temporal unit must be specified.")
}

/// The one unreadable-source failure: a `date`/`time`/`datetime` field holds
/// a value without the part the field selects — including a non-temporal
/// value, which is the same rejection rather than a silently absent field.
fn cannot_get(part: &str, value: &Value) -> Error {
    match value {
        Value::Temporal(t) => cannot_get_temporal(part, t),
        other => Error::ty(format!(
            "Cannot get the {part} of: {}",
            other.to_display_string()
        )),
    }
}

/// [`cannot_get`] for a value the caller already holds as a [`Temporal`] —
/// the readers' own rejection, spelled the same as the validator's.
pub(super) fn cannot_get_temporal(part: &str, t: &Temporal) -> Error {
    Error::ty(format!("Cannot get the {part} of: {}", t.iso_string()))
}

/// The temporals a map's `date`/`time`/`datetime` source fields hold,
/// resolved once by [`check_sources`]: the value supplying the date half (the
/// `date` field, else `datetime`) and the one supplying the time half (the
/// `time` field, else `datetime`). Presence and value are the same fact, so
/// no rule re-probes the fields with a second presence notion.
pub(super) struct Sources<'a> {
    pub(super) date: Option<&'a Temporal>,
    pub(super) time: Option<&'a Temporal>,
}

impl Sources<'_> {
    fn has_date(&self) -> bool {
        self.date.is_some()
    }

    fn has_time(&self) -> bool {
        self.time.is_some()
    }
}

/// The resolved form of a component-field map: the date form the map's own
/// fields selected and the source temporals supplying its halves.
pub(super) struct Validated<'a> {
    pub(super) form: Option<DateForm>,
    pub(super) sources: Sources<'a>,
}

/// Validate a component-field map for `kind` before it is constructed. The
/// answer carries the date form the map's own fields resolved (`None` when a
/// `date`/`datetime` source supplies the date or the kind carries no date)
/// and the source temporals the later rules and readers consume.
pub(super) fn validate(kind: ClockKind, m: &Fields) -> Result<Validated<'_>> {
    check_vocabulary(kind, m)?;
    // `check_vocabulary` already proved every key is one this kind consumes,
    // so emptiness is a value question.
    if !m.values().any(|value| !value.is_null()) {
        return Err(at_least_one_unit());
    }
    check_timezone(kind, m)?;
    let sources = check_sources(m)?;
    let form = check_date_fields(kind, m, &sources)?;
    check_time_fields(m, &sources)?;
    check_day_of_week(m)?;
    Ok(Validated { form, sources })
}

/// A clock-reading map is the `timezone` alone; beside any other field a
/// zoneless kind cannot consume it.
fn check_timezone(kind: ClockKind, m: &Fields) -> Result<()> {
    if !kind.has_zone()
        && present(m, "timezone")
        && m.iter()
            .any(|(key, value)| key != "timezone" && !value.is_null())
    {
        return Err(Error::ty(
            "Cannot assign time zone if also assigning other fields.",
        ));
    }
    Ok(())
}

/// The `date`/`time`/`datetime` source fields must hold the part they
/// select, and `datetime` cannot be combined with `date` or `time`. A
/// present non-temporal value is the typed unreadable-source rejection, never
/// a silently absent field the constructor then defaults over.
fn check_sources(m: &Fields) -> Result<Sources<'_>> {
    if present(m, "datetime") && (present(m, "date") || present(m, "time")) {
        return Err(Error::argument_error(
            "datetime cannot be selected together with date or time.",
        ));
    }
    let datetime = match present_value(m, "datetime") {
        None => None,
        Some(value) => match value.as_temporal() {
            Some(t) if t.has_date_and_time() => Some(t),
            // A source with one half only: name the part it lacks.
            Some(t) => {
                let part = if t.has_date() { "time" } else { "date" };
                return Err(cannot_get(part, value));
            }
            None => return Err(cannot_get("datetime", value)),
        },
    };
    let date = source_field(m, "date", Temporal::has_date)?;
    let time = source_field(m, "time", Temporal::has_time)?;
    Ok(Sources {
        date: datetime.or(date),
        time: datetime.or(time),
    })
}

/// The temporal a source field holds when it carries `part`, or the typed
/// unreadable-source rejection.
fn source_field<'a>(
    m: &'a Fields,
    key: &str,
    carries: fn(&Temporal) -> bool,
) -> Result<Option<&'a Temporal>> {
    match present_value(m, key) {
        None => Ok(None),
        Some(value) => match value.as_temporal() {
            Some(t) if carries(t) => Ok(Some(t)),
            _ => Err(cannot_get(key, value)),
        },
    }
}

/// The date-form rules: one form at a time, and a complete date when a time
/// is present. A `date`/`datetime` source supplies the missing components,
/// so its presence suspends both rules. `None` for a kind without a date.
fn check_date_fields(
    kind: ClockKind,
    m: &Fields,
    sources: &Sources<'_>,
) -> Result<Option<DateForm>> {
    if !kind.has_date() {
        return Ok(None);
    }
    let rule = date_form(m)?;
    let time_present = is_time_field_present(m) || sources.has_time();
    if sources.has_date() {
        return Ok(rule.map(|rule| rule.form));
    }
    if rule.is_none() && !present(m, "year") && !time_present {
        return Ok(None);
    }
    if !present(m, "year") {
        return Err(required_year());
    }
    // With a `year` and no form field the calendar form is the default; its
    // completeness rule applies once a time field is present.
    let rule = rule.unwrap_or(&DATE_FORM_RULES[0]);
    if let Some(day) = rule.day_name() {
        if present_alias(m, rule.day_keys) && !present_alias(m, rule.primary_keys) {
            return Err(Error::ty(format!(
                "{day} cannot be specified without {}",
                rule.primary_name()
            )));
        }
    }
    if time_present && rule.time_needs_both {
        if !present_alias(m, rule.primary_keys) {
            return Err(required(rule.primary_name()));
        }
        if let Some(day) = rule.day_name() {
            if !present_alias(m, rule.day_keys) {
                return Err(required(day));
            }
        }
    }
    Ok(Some(rule.form))
}

fn is_time_field_present(m: &Fields) -> bool {
    m.keys().any(|key| is_time_field(key) && present(m, key))
}

/// The time cascade: a coarser field must be present before a finer one,
/// and a `time`/`datetime` source supplies the whole cascade.
fn check_time_fields(m: &Fields, sources: &Sources<'_>) -> Result<()> {
    if !is_time_field_present(m) || sources.has_time() {
        return Ok(());
    }
    let subsecond =
        present(m, "millisecond") || present(m, "microsecond") || present(m, "nanosecond");
    if !present(m, "hour") {
        return Err(required("hour"));
    }
    if (present(m, "second") || subsecond) && !present(m, "minute") {
        return Err(required("minute"));
    }
    if subsecond && !present(m, "second") {
        return Err(required("second"));
    }
    Ok(())
}

/// Neo4j's day-of-week range is 1..=7; the weekday selector otherwise maps
/// modulo seven and would silently accept 0 and negatives.
fn check_day_of_week(m: &Fields) -> Result<()> {
    for key in DAY_OF_WEEK_KEYS {
        if let Some(Value::Int(day)) = m.get(*key) {
            if !(1..=7).contains(day) {
                return Err(Error::argument_error(format!(
                    "Invalid value for DayOfWeek (valid values 1 - 7): {day}"
                )));
            }
        }
    }
    Ok(())
}

/// Every key must be one the kind consumes. Non-`datetime` kinds do not
/// accept an epoch key, so the epoch form needs no separate kind check.
fn check_vocabulary(kind: ClockKind, m: &Fields) -> Result<()> {
    for key in m.keys() {
        if !accepts(kind, key) {
            return Err(Error::ty(format!("Not supported: {key}")));
        }
    }
    Ok(())
}

/// The epoch field a map's epoch form selects. Its keys are the one spelling
/// family (camelCase and lowercase), so a validator and a reader can never
/// disagree about which field names the instant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum EpochField {
    Seconds,
    Millis,
}

impl EpochField {
    /// The field's key family, canonical spelling first.
    pub(super) fn keys(self) -> &'static [&'static str] {
        match self {
            EpochField::Seconds => EPOCH_SECONDS_KEYS,
            EpochField::Millis => EPOCH_MILLIS_KEYS,
        }
    }

    /// The field's canonical spelling, for messages — the family's first key,
    /// so the spelling is never restated.
    pub(super) fn name(self) -> &'static str {
        self.keys()[0]
    }

    /// `(epoch seconds, subsecond nanos)` the field's integer names. The
    /// nanos are a `u32` by construction (a millisecond remainder is below
    /// 10^9), so the caller needs no clamp or unchecked cast.
    pub(super) fn split(self, value: i64) -> (i64, u32) {
        match self {
            EpochField::Seconds => (value, 0),
            EpochField::Millis => (
                value.div_euclid(1000),
                u32::try_from(value.rem_euclid(1000) * 1_000_000)
                    .expect("a millisecond remainder is below 10^9"),
            ),
        }
    }
}

/// Validate the epoch map form and answer which epoch field it selected: the
/// two spellings exclude each other, neither combines with a date/time
/// source, and at least one must be present. Plain component fields and
/// `timezone` stay legal overrides — `construct_map` applies them over the
/// instant's wall clock, so the validator's contract and the reader agree.
pub(super) fn validate_epoch(kind: ClockKind, m: &Fields) -> Result<EpochField> {
    check_vocabulary(kind, m)?;
    let seconds = present_alias(m, EPOCH_SECONDS_KEYS);
    let millis = present_alias(m, EPOCH_MILLIS_KEYS);
    if seconds && millis {
        return Err(Error::argument_error(
            "epochSeconds cannot be selected together with epochMillis.",
        ));
    }
    for key in ["date", "time", "datetime"] {
        if present(m, key) {
            return Err(Error::argument_error(format!(
                "epoch fields cannot be selected together with {key}."
            )));
        }
    }
    match (seconds, millis) {
        (true, false) => Ok(EpochField::Seconds),
        (false, true) => Ok(EpochField::Millis),
        _ => Err(at_least_one_unit()),
    }
}
