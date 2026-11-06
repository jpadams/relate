//! The params builder's argument decoders (ADR-0502 §4) — the one place
//! a putter's C arguments become checked Rust components: calendar and
//! clock components, zone arguments, vector dtypes and coordinate
//! buffers. Their own module because the builder model (containers,
//! entries, the budget) and the eighteen putter entry points stay in
//! `params` — the decode policy is a separate stratum of the file.

#![allow(non_camel_case_types)]

use std::os::raw::c_char;

use chrono::{FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use relate_core::temporal::{zone_region, ZoneError, MAX_OFFSET_SECONDS};
use relate_model::value::VectorCoordKind;

use super::MAX_PARAM_NAME_BYTES;
use crate::abi::read_cstr_bounded;

/// A calendar date from components; out of range refuses.
pub(super) fn calendar_date(year: i64, month: i64, day: i64) -> Result<NaiveDate, String> {
    let date = i32::try_from(year).ok().and_then(|y| {
        NaiveDate::from_ymd_opt(y, u32::try_from(month).ok()?, u32::try_from(day).ok()?)
    });
    date.ok_or_else(|| format!("date components out of range: {year}-{month}-{day}"))
}

/// A wall-clock time from components — hour 0-23, minute 0-59, second
/// 0-59, nanosecond 0-999999999; out of range refuses (chrono's leap-second
/// nanoseconds included).
pub(super) fn wall_time(
    hour: i64,
    minute: i64,
    second: i64,
    nanos: i64,
) -> Result<NaiveTime, String> {
    let part = |v: i64, max: i64| (0..=max).contains(&v).then_some(v as u32);
    let time = match (
        part(hour, 23),
        part(minute, 59),
        part(second, 59),
        part(nanos, 999_999_999),
    ) {
        (Some(h), Some(mi), Some(s), Some(ns)) => NaiveTime::from_hms_nano_opt(h, mi, s, ns),
        _ => None,
    };
    time.ok_or_else(|| {
        format!(
            "time components out of range: {hour}:{minute}:{second}.{nanos} \
             (hour 0-23, minute 0-59, second 0-59, nanosecond 0-999999999)"
        )
    })
}

/// A UTC offset in seconds, bounded as the engine bounds a zone offset
/// (`±18:00`) — the magnitude compared through `unsigned_abs`, the
/// canonical layer's idiom for a user-controlled field.
pub(super) fn fixed_offset(offset_seconds: i64) -> Result<FixedOffset, String> {
    i32::try_from(offset_seconds)
        .ok()
        .filter(|_| offset_seconds.unsigned_abs() <= MAX_OFFSET_SECONDS as u64)
        .and_then(FixedOffset::east_opt)
        .ok_or_else(|| format!("UTC offset out of range (±18:00): {offset_seconds} seconds"))
}

/// The zone argument of a zoned putter: NULL selects the offset; otherwise
/// an IANA region id (`Europe/Stockholm`) — the zones a DATETIME keeps and
/// the walker reports back. Offset-only spellings pass as the offset.
///
/// # Safety
/// `zone` must be null or a NUL-terminated string.
pub(super) unsafe fn zone_arg(zone: *const c_char) -> Result<Option<String>, String> {
    if zone.is_null() {
        return Ok(None);
    }
    // SAFETY: `zone` is non-null and NUL-terminated per the contract;
    // the bounded read refuses an over-cap zone before copying it.
    let zone = unsafe { read_cstr_bounded(zone, MAX_PARAM_NAME_BYTES, "zone name") }?;
    if zone_region(Some(&zone)).is_none() {
        return Err(format!(
            "zone '{zone}' is not an IANA region id; pass an offset-only zone as offset_seconds"
        ));
    }
    Ok(Some(zone))
}

/// The one spelling of a zone-resolution refusal.
pub(super) fn zone_refusal(zone: &str, e: ZoneError) -> String {
    match e {
        ZoneError::UnknownZone => format!("unknown time zone '{zone}'"),
        ZoneError::NonexistentWallTime => {
            format!("the wall clock does not occur in time zone '{zone}' (a DST gap)")
        }
    }
}

/// A vector's dtype argument: the engine's canonical coordinate-type name,
/// the spelling `relate_value_vector_dtype` returns.
///
/// # Safety
/// `dtype` must be null or a NUL-terminated string.
pub(super) unsafe fn vector_dtype(dtype: *const c_char) -> Result<VectorCoordKind, String> {
    // SAFETY: forwarded contract.
    let name = unsafe { read_cstr_bounded(dtype, MAX_PARAM_NAME_BYTES, "vector dtype") }?;
    VectorCoordKind::from_canonical_name(&name)
        .ok_or_else(|| format!("unknown vector dtype '{name}'"))
}

/// A vector's coordinates, copied out of the caller's buffer; NULL is
/// allowed only for dimension 0. The copy happens inside the call, so no
/// borrow of caller memory outlives it.
///
/// # Safety
/// `values` must be null or point to `dim` readable elements for the
/// duration of this call.
pub(super) unsafe fn coordinates<T: Copy>(values: *const T, dim: usize) -> Result<Vec<T>, String> {
    if dim == 0 {
        return Ok(Vec::new());
    }
    if values.is_null() {
        return Err("null vector coordinates".to_string());
    }
    // SAFETY: `values` is non-null and points to `dim` elements readable
    // for this call per the contract; the slice dies at the end of this
    // statement, after `to_vec` copies it.
    Ok(unsafe { std::slice::from_raw_parts(values, dim) }.to_vec())
}

/// The local date-time a DATETIME / LOCAL_DATETIME putter names.
pub(super) fn local_datetime(
    [year, month, day, hour, minute, second, nanosecond]: [i64; 7],
) -> Result<NaiveDateTime, String> {
    Ok(calendar_date(year, month, day)?.and_time(wall_time(hour, minute, second, nanosecond)?))
}
