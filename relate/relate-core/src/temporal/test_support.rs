//! Shared builders for the temporal unit tests.

use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveDateTime};

use crate::Value;

/// A UTC naive datetime from its components — the clock tests' instant
/// builder, so the pin block is written once.
pub(super) fn naive_utc(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_opt(hour, minute, second))
        .expect("a valid test instant")
}

/// A `Value::Map` from `(key, value)` pairs — the one component-map builder
/// the constructor and duration tests share.
pub(super) fn component_map(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect::<BTreeMap<_, _>>()
            .into(),
    )
}
