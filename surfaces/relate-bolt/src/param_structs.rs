//! Decoding of Bolt parameter structs into engine values (ADR-0177).
//!
//! [`params_to_map`] is the module's public entry point and the RUN path's
//! conversion: it turns the params field into the engine's parameter map,
//! seeding each top-level parameter's path as `$name`. `Pack::to_mdb_at`
//! routes each `Pack::Struct` through the tag decoders below: the nine
//! temporal, duration, and point tags decode faithfully, and every other
//! struct or malformed field fails the RUN with a path-qualified message
//! instead of silently becoming `null`. The DateTime tags are the UTC pair
//! `0x49`/`0x69` under the connection's `utc` patch (ADR-0329) or on a 5.x
//! connection, and the legacy `0x46`/`0x66` otherwise (ADR-0484 decision
//! 7 — parameters and results alike, the one shared
//! [`Dialect::utc_datetime_structures`] derivation). PackStream Bytes decode to
//! the engine's statement-scoped byte array (ADR-0328).
//!
//! The parameter path is accumulated as borrowed segments while descending
//! (pushed before a child, popped after it) and rendered into the `$…`
//! string only when a failure is built, so a successful conversion never
//! materializes a per-element path string. The 0x66 wall clock resolves
//! through the engine's one IANA rule, `zone_offset_at` (ADR-0178).

use std::collections::BTreeMap;

use chrono::{DateTime, Duration as ChronoDur, NaiveDateTime, NaiveTime};
use relate::{Temporal, Value};
use relate_core::temporal::{zone_offset_at, zone_offset_at_utc, ZoneError};
use relate_core::PointValue;

use crate::dialect::Dialect;
use crate::packstream::Pack;
use crate::value_structs::{
    DATE, DATE_TIME, DATE_TIME_UTC, DATE_TIME_ZONE_ID, DATE_TIME_ZONE_ID_UTC, DURATION,
    LOCAL_DATE_TIME, LOCAL_TIME, NANOS_PER_SECOND, POINT_2D, POINT_3D, TIME,
};

// A sub-second component of 0x46/0x66/0x64.
const MAX_SUBSECOND: i64 = 999_999_999;
// Nanoseconds since midnight for 0x54/0x74 (23:59:59.999999999).
const MAX_TIME_OF_DAY: i64 = 86_399_999_999_999;

/// Convert a RUN's params field into engine values (ADR-0177), the one
/// parameter-conversion entry point the RUN handler calls. The field is
/// optional at the caller: an absent field stays the empty map, while a
/// present field that is not a `Pack::Map` or whose top-level key is not a
/// string is an `Err` — never a silently dropped parameter. Nested values
/// (and their non-string keys) are `to_mdb_at`'s job; the top-level
/// parameter `name` seeds the path as `$name`. `dialect` is the
/// connection's negotiated dialect (ADR-0329, ADR-0484), which selects the
/// DateTime pair (decision 7).
pub(crate) fn params_to_map(
    p: &Pack,
    dialect: Dialect,
) -> Result<BTreeMap<String, relate::Value>, String> {
    let Pack::Map(entries) = p else {
        return Err(format!(
            "unsupported parameter value for $: {} (RUN params must be a map)",
            p.kind_name()
        ));
    };
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        let ks = k.string_map_key(&ParamPath::root())?;
        let mut path = ParamPath::param(ks);
        m.insert(ks.to_string(), v.to_mdb_at(&mut path, dialect)?);
    }
    Ok(m)
}

impl Pack {
    /// Convert a wire value into an engine value, failing on anything the
    /// engine cannot represent (ADR-0177): a struct tag outside the
    /// supported set, or a non-string map key at any depth. The `String` is
    /// the parameter-path-qualified message from the ADR's failure contract.
    /// No `patch_bolt` patch applies and the version is Bolt 4.4 — the
    /// legacy DateTime tags.
    ///
    /// Crate-internal and test-only (ADR-0179): the RUN path converts
    /// through [`params_to_map`], which needs the per-parameter path; this
    /// root-path wrapper is the whole-value convenience the in-crate tests
    /// call.
    #[cfg(test)]
    pub(crate) fn to_mdb(&self) -> Result<relate::Value, String> {
        self.to_mdb_at(&mut ParamPath::root(), Dialect::plain_44())
    }

    /// The path-aware `to_mdb`: `path` is the parameter path of `self` (`$`,
    /// `$name`, `.key`, `[i]`; ADR-0177) named in every error. Recursion
    /// extends the path for list elements (`[i]`) and map entries (`.key`)
    /// with borrowed segments — push before descending, pop on return — so
    /// the success path allocates no per-element or per-entry path string;
    /// only the failure constructors render it. `dialect` selects the
    /// DateTime tags (ADR-0329, ADR-0484 decision 7).
    pub(crate) fn to_mdb_at<'a, 'p>(
        &'a self,
        path: &mut ParamPath<'p>,
        dialect: Dialect,
    ) -> Result<relate::Value, String>
    where
        'a: 'p,
    {
        use relate::Value;
        Ok(match self {
            Pack::Null => Value::Null,
            Pack::Bool(b) => Value::Bool(*b),
            Pack::Int(i) => Value::Int(*i),
            Pack::Float(f) => Value::Float(*f),
            Pack::String(s) => Value::String(s.as_str().into()),
            Pack::List(xs) => {
                let mut out = Vec::with_capacity(xs.len());
                for (i, x) in xs.iter().enumerate() {
                    path.push(PathSeg::Index(i));
                    let value = x.to_mdb_at(path, dialect);
                    path.pop();
                    out.push(value?);
                }
                Value::List(out.into())
            }
            Pack::Map(m) => {
                let mut map = std::collections::BTreeMap::new();
                for (k, v) in m {
                    let ks = k.string_map_key(path)?;
                    path.push(PathSeg::Key(ks));
                    let value = v.to_mdb_at(path, dialect);
                    path.pop();
                    map.insert(ks.to_string(), value?);
                }
                Value::Map(map.into())
            }
            // ADR-0328: the statement-scoped engine byte array.
            Pack::Bytes(b) => Value::Bytes(b.as_slice().into()),
            Pack::Struct { tag, fields } => struct_to_value(*tag, fields, path, dialect)?,
        })
    }

    /// The Pack variant name used in ADR-0177 parameter error messages:
    /// `Null`, `Bool`, `Int`, `Float`, `String`, `List`, `Map`, `Bytes`, or
    /// `Struct(0xNN)` with two uppercase hex digits.
    pub(crate) fn kind_name(&self) -> String {
        match self {
            Pack::Null => "Null".into(),
            Pack::Bool(_) => "Bool".into(),
            Pack::Int(_) => "Int".into(),
            Pack::Float(_) => "Float".into(),
            Pack::String(_) => "String".into(),
            Pack::List(_) => "List".into(),
            Pack::Map(_) => "Map".into(),
            Pack::Bytes(_) => "Bytes".into(),
            Pack::Struct { tag, .. } => struct_tag_name(*tag),
        }
    }

    /// The string of a map key, or the ADR-0177 failure when the engine
    /// cannot represent the key: the key's `kind_name` with the map's own
    /// `path` named. The single source of the non-string-key message.
    pub(crate) fn string_map_key<'a>(&'a self, path: &ParamPath) -> Result<&'a str, String> {
        self.as_str().ok_or_else(|| {
            format!(
                "unsupported parameter map key for {}: {} (map keys must be strings)",
                path.render(),
                self.kind_name()
            )
        })
    }
}

/// One step of a parameter path (ADR-0177 item 6): `.key` for a map entry,
/// `[i]` for a list element. A key segment borrows the wire key, so pushing
/// a segment allocates nothing.
enum PathSeg<'a> {
    Key(&'a str),
    Index(usize),
}

/// The path of the value under conversion. It is accumulated as segments on
/// the way down and rendered only when an error is built; `root` is the
/// top-level parameter name (`$name`) or empty for the params root (`$`).
pub(crate) struct ParamPath<'a> {
    root: &'a str,
    segs: Vec<PathSeg<'a>>,
}

impl<'a> ParamPath<'a> {
    /// The `$` root: a whole value (`to_mdb`) or the RUN params field.
    pub(crate) fn root() -> Self {
        Self {
            root: "",
            segs: Vec::new(),
        }
    }

    /// A top-level parameter path `$name`.
    pub(crate) fn param(name: &'a str) -> Self {
        Self {
            root: name,
            segs: Vec::new(),
        }
    }

    fn push(&mut self, seg: PathSeg<'a>) {
        self.segs.push(seg);
    }

    fn pop(&mut self) {
        self.segs.pop();
    }

    /// Render the path exactly as the ADR-0177 message templates name it:
    /// `$`, `$name`, `.key`, `[i]`.
    fn render(&self) -> String {
        let mut out = String::new();
        out.push('$');
        out.push_str(self.root);
        for seg in &self.segs {
            match seg {
                PathSeg::Key(k) => {
                    out.push('.');
                    out.push_str(k);
                }
                PathSeg::Index(i) => {
                    use std::fmt::Write;
                    let _ = write!(out, "[{i}]");
                }
            }
        }
        out
    }
}

/// Decode one parameter struct. The DateTime pair follows the one shared
/// derivation ([`Dialect::utc_datetime_structures`]): the UTC pair
/// `0x49`/`0x69` under the `utc` patch or on a 5.x connection, the legacy
/// `0x46`/`0x66` otherwise; the other pair is an unsupported struct either
/// way (ADR-0329, ADR-0484 decision 7).
pub(crate) fn struct_to_value<'a, 'p>(
    tag: u8,
    fields: &'a [Pack],
    path: &'a ParamPath<'p>,
    dialect: Dialect,
) -> Result<relate::Value, String> {
    match (tag, dialect.utc_datetime_structures()) {
        (DATE, _) => date_to_value(fields, path),
        (TIME, _) => time_to_value(fields, path),
        (LOCAL_TIME, _) => local_time_to_value(fields, path),
        (DATE_TIME, false) => datetime_to_value(fields, path),
        (DATE_TIME_ZONE_ID, false) => datetime_zone_id_to_value(fields, path),
        (DATE_TIME_UTC, true) => datetime_utc_to_value(fields, path),
        (DATE_TIME_ZONE_ID_UTC, true) => datetime_zone_id_utc_to_value(fields, path),
        (LOCAL_DATE_TIME, _) => local_datetime_to_value(fields, path),
        (DURATION, _) => duration_to_value(fields, path),
        (POINT_2D, _) => point_2d_to_value(fields, path),
        (POINT_3D, _) => point_3d_to_value(fields, path),
        _ => Err(format!(
            "unsupported parameter value for {}: {}",
            path.render(),
            struct_tag_name(tag)
        )),
    }
}

/// The one rendering of the `Struct(0xNN)` tag vocabulary (two uppercase hex
/// digits) shared by every ADR-0177 message: `kind_name`, the unsupported-tag
/// fallback, and the field-error builders.
fn struct_tag_name(tag: u8) -> String {
    format!("Struct(0x{tag:02X})")
}

/// One parameter struct under conversion: its tag, its wire fields, and the
/// path of the struct itself. Each decoder builds it once; `new` enforces the
/// tag's arity, and every accessor then names only the field index.
struct Fields<'a, 'p> {
    tag: u8,
    fields: &'a [Pack],
    path: &'a ParamPath<'p>,
}

impl<'a, 'p> Fields<'a, 'p> {
    fn new(
        tag: u8,
        fields: &'a [Pack],
        path: &'a ParamPath<'p>,
        expected: usize,
    ) -> Result<Self, String> {
        if fields.len() != expected {
            return Err(invalid(
                path,
                format!(
                    "{} has {} fields, expected {expected}",
                    struct_tag_name(tag),
                    fields.len()
                ),
            ));
        }
        Ok(Self { tag, fields, path })
    }

    fn int(&self, index: usize) -> Result<i64, String> {
        match &self.fields[index] {
            Pack::Int(i) => Ok(*i),
            other => Err(self.wrong_type(index, "Int", other)),
        }
    }

    fn float(&self, index: usize) -> Result<f64, String> {
        match &self.fields[index] {
            Pack::Float(f) => Ok(*f),
            other => Err(self.wrong_type(index, "Float", other)),
        }
    }

    fn string(&self, index: usize) -> Result<&'a str, String> {
        match &self.fields[index] {
            Pack::String(s) => Ok(s),
            other => Err(self.wrong_type(index, "String", other)),
        }
    }

    fn subsecond(&self, index: usize) -> Result<u32, String> {
        let nanos = self.int(index)?;
        if !(0..=MAX_SUBSECOND).contains(&nanos) {
            return Err(self.out_of_range(index, nanos));
        }
        Ok(nanos as u32)
    }

    fn nanos_to_time(&self, index: usize) -> Result<NaiveTime, String> {
        let nanos = self.int(index)?;
        if !(0..=MAX_TIME_OF_DAY).contains(&nanos) {
            return Err(self.out_of_range(index, nanos));
        }
        NaiveTime::from_num_seconds_from_midnight_opt(
            (nanos / NANOS_PER_SECOND) as u32,
            (nanos % NANOS_PER_SECOND) as u32,
        )
        .ok_or_else(|| self.out_of_range(index, nanos))
    }

    fn epoch_to_naive(&self, seconds: i64, nanos: u32) -> Result<NaiveDateTime, String> {
        DateTime::from_timestamp(seconds, nanos)
            .map(|dt| dt.naive_utc())
            .ok_or_else(|| self.out_of_range(0, seconds))
    }

    fn i32_offset(&self, index: usize) -> Result<i32, String> {
        let raw = self.int(index)?;
        i32::try_from(raw).map_err(|_| self.out_of_range(index, raw))
    }

    fn point_srid(&self, srid: i64, expected: u8) -> Result<i32, String> {
        match srid_dimension(srid) {
            None => Err(invalid(
                self.path,
                format!("unknown srid {srid}: expected one of 4326, 4979, 7203, 9157"),
            )),
            Some(dimension) if dimension != expected => Err(invalid(
                self.path,
                format!(
                    "srid {srid} does not match the coordinate dimension of {}",
                    struct_tag_name(self.tag)
                ),
            )),
            Some(_) => Ok(srid as i32),
        }
    }

    fn wrong_type(&self, index: usize, expected: &str, got: &Pack) -> String {
        invalid(
            self.path,
            format!(
                "field {index} of {} must be {expected}, got {}",
                struct_tag_name(self.tag),
                got.kind_name()
            ),
        )
    }

    fn out_of_range(&self, index: usize, value: impl std::fmt::Display) -> String {
        invalid(
            self.path,
            format!(
                "field {index} of {} is out of range: {value}",
                struct_tag_name(self.tag)
            ),
        )
    }
}

// 0x44 Date: [days: Int] — 1970-01-01 + days in the proleptic Gregorian
// calendar; a date outside chrono's representable range is rejected.
fn date_to_value<'a, 'p>(fields: &'a [Pack], path: &'a ParamPath<'p>) -> Result<Value, String> {
    let f = Fields::new(DATE, fields, path, 1)?;
    let days = f.int(0)?;
    let seconds = days
        .checked_mul(86_400)
        .ok_or_else(|| f.out_of_range(0, days))?;
    let date = DateTime::from_timestamp(seconds, 0)
        .map(|dt| dt.date_naive())
        .ok_or_else(|| f.out_of_range(0, days))?;
    Ok(Value::Temporal(Temporal::Date(date)))
}

// 0x54 Time: [nanoseconds: Int, tz_offset_seconds: Int] — nanoseconds since
// midnight, plus the offset the value is rendered at.
fn time_to_value<'a, 'p>(fields: &'a [Pack], path: &'a ParamPath<'p>) -> Result<Value, String> {
    let f = Fields::new(TIME, fields, path, 2)?;
    let time = f.nanos_to_time(0)?;
    let offset = f.i32_offset(1)?;
    Ok(Value::Temporal(Temporal::Time { time, offset }))
}

// 0x74 LocalTime: [nanoseconds: Int].
fn local_time_to_value<'a, 'p>(
    fields: &'a [Pack],
    path: &'a ParamPath<'p>,
) -> Result<Value, String> {
    let f = Fields::new(LOCAL_TIME, fields, path, 1)?;
    let time = f.nanos_to_time(0)?;
    Ok(Value::Temporal(Temporal::LocalTime(time)))
}

// 0x46 legacy DateTime: [seconds: Int, nanoseconds: Int,
// tz_offset_seconds: Int]. The wire carries local epoch seconds, so the UTC
// instant is `seconds - offset`; an explicit offset is never `zone: Some("Z")`
// (ADR-0177 item 3).
fn datetime_to_value<'a, 'p>(fields: &'a [Pack], path: &'a ParamPath<'p>) -> Result<Value, String> {
    let f = Fields::new(DATE_TIME, fields, path, 3)?;
    let seconds = f.int(0)?;
    let nanos = f.subsecond(1)?;
    let offset = f.i32_offset(2)?;
    let utc_seconds = seconds
        .checked_sub(i64::from(offset))
        .ok_or_else(|| f.out_of_range(0, seconds))?;
    let utc = f.epoch_to_naive(utc_seconds, nanos)?;
    Ok(Value::Temporal(Temporal::DateTime {
        utc,
        offset,
        zone: None,
    }))
}

// 0x66 legacy DateTimeZoneId: [seconds: Int, nanoseconds: Int, tz_id: String].
// The wire carries the wall clock in `tz_id`; resolve it through the engine's
// one IANA rule — unique offset, else the earliest on an overlap, and a DST
// gap is rejected (ADR-0177 item 4, ADR-0178).
fn datetime_zone_id_to_value<'a, 'p>(
    fields: &'a [Pack],
    path: &'a ParamPath<'p>,
) -> Result<Value, String> {
    let f = Fields::new(DATE_TIME_ZONE_ID, fields, path, 3)?;
    let seconds = f.int(0)?;
    let nanos = f.subsecond(1)?;
    let zone_id = f.string(2)?;
    let wall = f.epoch_to_naive(seconds, nanos)?;
    let offset =
        zone_offset_at(zone_id, wall).map_err(|e| zone_resolution_error(path, zone_id, wall, e))?;
    let utc = wall
        .checked_sub_signed(ChronoDur::seconds(i64::from(offset)))
        .ok_or_else(|| f.out_of_range(0, seconds))?;
    Ok(Value::Temporal(Temporal::DateTime {
        utc,
        offset,
        zone: Some(zone_id.to_string()),
    }))
}

// 0x49 DateTimeUTC (utc patch or a 5.x connection, ADR-0329, ADR-0484
// decision 7): [seconds: Int, nanoseconds: Int, tz_offset_seconds: Int]. The
// wire carries UTC epoch seconds, so the instant is read directly; like 0x46
// the offset form is `zone: None`.
fn datetime_utc_to_value<'a, 'p>(
    fields: &'a [Pack],
    path: &'a ParamPath<'p>,
) -> Result<Value, String> {
    let f = Fields::new(DATE_TIME_UTC, fields, path, 3)?;
    let seconds = f.int(0)?;
    let nanos = f.subsecond(1)?;
    let offset = f.i32_offset(2)?;
    let utc = f.epoch_to_naive(seconds, nanos)?;
    Ok(Value::Temporal(Temporal::DateTime {
        utc,
        offset,
        zone: None,
    }))
}

// 0x69 DateTimeZoneIdUTC (utc patch or a 5.x connection, ADR-0329,
// ADR-0484 decision 7): [seconds: Int, nanoseconds: Int, tz_id: String]. The
// wire carries the UTC instant, so the zone's offset at that instant is
// unambiguous; only an unknown zone fails.
fn datetime_zone_id_utc_to_value<'a, 'p>(
    fields: &'a [Pack],
    path: &'a ParamPath<'p>,
) -> Result<Value, String> {
    let f = Fields::new(DATE_TIME_ZONE_ID_UTC, fields, path, 3)?;
    let seconds = f.int(0)?;
    let nanos = f.subsecond(1)?;
    let zone_id = f.string(2)?;
    let utc = f.epoch_to_naive(seconds, nanos)?;
    let offset = zone_offset_at_utc(zone_id, utc)
        .map_err(|e| zone_resolution_error(path, zone_id, utc, e))?;
    Ok(Value::Temporal(Temporal::DateTime {
        utc,
        offset,
        zone: Some(zone_id.to_string()),
    }))
}

// 0x64 LocalDateTime: [seconds: Int, nanoseconds: Int] — epoch seconds with
// no zone.
fn local_datetime_to_value<'a, 'p>(
    fields: &'a [Pack],
    path: &'a ParamPath<'p>,
) -> Result<Value, String> {
    let f = Fields::new(LOCAL_DATE_TIME, fields, path, 2)?;
    let seconds = f.int(0)?;
    let nanos = f.subsecond(1)?;
    let local = f.epoch_to_naive(seconds, nanos)?;
    Ok(Value::Temporal(Temporal::LocalDateTime(local)))
}

// 0x45 Duration: [months: Int, days: Int, seconds: Int, nanoseconds: Int].
// The sub-second field is a signed remainder and is not bounded by the
// layout; it folds into the i128 `nanos` without overflow.
fn duration_to_value<'a, 'p>(fields: &'a [Pack], path: &'a ParamPath<'p>) -> Result<Value, String> {
    let f = Fields::new(DURATION, fields, path, 4)?;
    let months = f.int(0)?;
    let days = f.int(1)?;
    let seconds = f.int(2)?;
    let nanoseconds = f.int(3)?;
    let nanos = i128::from(seconds) * i128::from(NANOS_PER_SECOND) + i128::from(nanoseconds);
    Ok(Value::Temporal(Temporal::Duration {
        months,
        days,
        nanos,
    }))
}

/// The coordinate dimension an SRID names: `4326`/`7203` are 2D,
/// `4979`/`9157` are 3D, and every other SRID is unknown (ADR-0177 item 5).
fn srid_dimension(srid: i64) -> Option<u8> {
    match srid {
        4326 | 7203 => Some(2),
        4979 | 9157 => Some(3),
        _ => None,
    }
}

// 0x58 Point (2D): [srid: Int, x: Float, y: Float]; the SRID must be 2D.
fn point_2d_to_value<'a, 'p>(fields: &'a [Pack], path: &'a ParamPath<'p>) -> Result<Value, String> {
    let f = Fields::new(POINT_2D, fields, path, 3)?;
    let srid = f.point_srid(f.int(0)?, 2)?;
    let x = f.float(1)?;
    let y = f.float(2)?;
    Ok(Value::Point(PointValue {
        srid,
        x,
        y,
        z: None,
    }))
}

// 0x59 Point (3D): [srid: Int, x: Float, y: Float, z: Float]; the SRID must
// be 3D.
fn point_3d_to_value<'a, 'p>(fields: &'a [Pack], path: &'a ParamPath<'p>) -> Result<Value, String> {
    let f = Fields::new(POINT_3D, fields, path, 4)?;
    let srid = f.point_srid(f.int(0)?, 3)?;
    let x = f.float(1)?;
    let y = f.float(2)?;
    let z = f.float(3)?;
    Ok(Value::Point(PointValue {
        srid,
        x,
        y,
        z: Some(z),
    }))
}

/// The ADR-0177 zone-resolution failure, shared by the 0x66 wall-clock and
/// 0x69 UTC decoders (ADR-0340): an unknown zone, or — reachable only from
/// a wall clock, since an instant is never in a DST gap (ADR-0340) — a
/// nonexistent local time.
fn zone_resolution_error(
    path: &ParamPath,
    zone_id: &str,
    at: NaiveDateTime,
    e: ZoneError,
) -> String {
    match e {
        ZoneError::UnknownZone => unknown_zone(path, zone_id),
        ZoneError::NonexistentWallTime => invalid(
            path,
            format!(
                "ambiguous local time {} in timezone '{zone_id}'",
                at.format("%Y-%m-%dT%H:%M:%S%.f")
            ),
        ),
    }
}

/// The ADR-0177 unknown-`tz_id` failure, reached from the shared
/// [`zone_resolution_error`] for both the 0x66 and 0x69 decoders.
fn unknown_zone(path: &ParamPath, zone_id: &str) -> String {
    invalid(path, format!("unknown timezone '{zone_id}'"))
}

fn invalid(path: &ParamPath, detail: impl std::fmt::Display) -> String {
    format!(
        "invalid parameter structure for {}: {detail}",
        path.render()
    )
}

#[cfg(test)]
#[path = "param_structs_tests.rs"]
mod tests;
