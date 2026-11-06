//! Shared temporal fixtures: the DST fall-back overlap pair every
//! distinct/equality test pins (TCK TemporalCompare; ADR-0446), as a
//! [`Value`] constructor for in-engine tests and as Cypher literals for
//! end-to-end ones.

use relate_core::{Temporal, Value};

/// The Cypher literal of the first half of the overlap (`+02:00`).
pub const DST_OVERLAP_FIRST: &str = "datetime('2025-10-26T02:30:00+02:00[Europe/Stockholm]')";

/// The Cypher literal of the second half of the overlap (`+01:00`).
pub const DST_OVERLAP_SECOND: &str = "datetime('2025-10-26T02:30:00+01:00[Europe/Stockholm]')";

/// One half of the Europe/Stockholm DST fall-back overlap at 2025-10-26
/// 02:30 local: `=` holds between the halves (equal wall clock and zone),
/// yet their UTC instants differ, so they are two grouping keys.
pub fn dst_overlap_half(second: bool) -> Value {
    let local = chrono::NaiveDate::from_ymd_opt(2025, 10, 26)
        .and_then(|d| d.and_hms_opt(2, 30, 0))
        .expect("a valid wall clock");
    let offset = if second { 3600 } else { 7200 };
    Value::Temporal(Temporal::DateTime {
        utc: local - chrono::Duration::seconds(i64::from(offset)),
        offset,
        zone: Some("Europe/Stockholm".to_string()),
    })
}
