//! Unit tests for ADR-0177 parameter struct decoding, the `utc` patch's
//! DateTime tags (ADR-0329), the same UTC pair unconditional on a 5.4
//! connection (ADR-0484 decision 7) and Bytes parameters (ADR-0328).
//!
//! Each worked example from the ADR's conversion table is pinned by the
//! variant and components it names, plus the `iso_string` the table records;
//! every rejection case is pinned by the exact failure-contract message.

use super::*;
use chrono::NaiveDate;

fn decode(tag: u8, fields: Vec<Pack>) -> Result<Value, String> {
    struct_to_value(tag, &fields, &ParamPath::param("p"), Dialect::plain_44())
}

/// `decode` on a connection that negotiated the `utc` patch (ADR-0329):
/// the 4.4 spelling of the UTC DateTime pair.
fn decode_utc(tag: u8, fields: Vec<Pack>) -> Result<Value, String> {
    struct_to_value(tag, &fields, &ParamPath::param("p"), Dialect::utc_44())
}

/// `decode` on a 5.4 connection (ADR-0484 decision 7): the UTC DateTime
/// pair with no patch to negotiate.
fn decode_5_4(tag: u8, fields: Vec<Pack>) -> Result<Value, String> {
    struct_to_value(tag, &fields, &ParamPath::param("p"), Dialect::at_5_4())
}

fn temporal(v: &Value) -> &Temporal {
    match v {
        Value::Temporal(t) => t,
        other => panic!("expected a Temporal value, got {other:?}"),
    }
}

fn point(v: &Value) -> &PointValue {
    match v {
        Value::Point(p) => p,
        other => panic!("expected a Point value, got {other:?}"),
    }
}

// Parse a test expectation; the ADR pins `iso_string` output, and parsing the
// same text back keeps the expectation independent of chrono's constructors.
fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").unwrap()
}

// --- Worked examples (ADR-0177 item 2) ------------------------------------

#[test]
fn date_decodes_worked_example() {
    let v = decode(0x44, vec![Pack::Int(0)]).unwrap();
    assert_eq!(
        v,
        Value::Temporal(Temporal::Date(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()))
    );
    assert_eq!(temporal(&v).iso_string(), "1970-01-01");
}

#[test]
fn time_decodes_worked_example() {
    let v = decode(0x54, vec![Pack::Int(45_000_000_000_000), Pack::Int(3600)]).unwrap();
    match temporal(&v) {
        Temporal::Time { time, offset } => {
            assert_eq!(*time, NaiveTime::from_hms_opt(12, 30, 0).unwrap());
            assert_eq!(*offset, 3600);
        }
        other => panic!("expected Time, got {other:?}"),
    }
}

#[test]
fn local_time_decodes_worked_example() {
    let v = decode(0x74, vec![Pack::Int(45_000_000_000_000)]).unwrap();
    assert_eq!(
        v,
        Value::Temporal(Temporal::LocalTime(
            NaiveTime::from_hms_opt(12, 30, 0).unwrap()
        ))
    );
}

#[test]
fn legacy_datetime_decodes_worked_example() {
    // Bolt docs 0x46 [8100, 42, 3600] = 1970-01-01T02:15:00.000000042+01:00.
    let v = decode(0x46, vec![Pack::Int(8100), Pack::Int(42), Pack::Int(3600)]).unwrap();
    match temporal(&v) {
        Temporal::DateTime { utc, offset, zone } => {
            assert_eq!(*utc, dt("1970-01-01T01:15:00.000000042"));
            assert_eq!(*offset, 3600);
            assert_eq!(*zone, None);
        }
        other => panic!("expected DateTime, got {other:?}"),
    }
    assert_eq!(
        temporal(&v).iso_string(),
        "1970-01-01T02:15:00.000000042+01:00"
    );
}

#[test]
fn legacy_datetime_with_zero_offset_stays_zone_none() {
    // An explicit wire offset is never synthesized as zone Some("Z"); the
    // zero offset still renders with the Z suffix (ADR-0177 item 3).
    let v = decode(0x46, vec![Pack::Int(4500), Pack::Int(42), Pack::Int(0)]).unwrap();
    match temporal(&v) {
        Temporal::DateTime { utc, offset, zone } => {
            assert_eq!(*utc, dt("1970-01-01T01:15:00.000000042"));
            assert_eq!(*offset, 0);
            assert_eq!(*zone, None);
        }
        other => panic!("expected DateTime, got {other:?}"),
    }
    assert_eq!(temporal(&v).iso_string(), "1970-01-01T01:15:00.000000042Z");
}

#[test]
fn zoned_datetime_decodes_worked_example() {
    // Bolt docs 0x66 [8100, 42, "Europe/Paris"] is the same instant with the
    // IANA zone attached.
    let v = decode(
        0x66,
        vec![
            Pack::Int(8100),
            Pack::Int(42),
            Pack::String("Europe/Paris".into()),
        ],
    )
    .unwrap();
    match temporal(&v) {
        Temporal::DateTime { utc, offset, zone } => {
            assert_eq!(*utc, dt("1970-01-01T01:15:00.000000042"));
            assert_eq!(*offset, 3600);
            assert_eq!(zone.as_deref(), Some("Europe/Paris"));
        }
        other => panic!("expected DateTime, got {other:?}"),
    }
    assert_eq!(
        temporal(&v).iso_string(),
        "1970-01-01T02:15:00.000000042+01:00[Europe/Paris]"
    );
}

#[test]
fn local_datetime_decodes_worked_example() {
    let v = decode(0x64, vec![Pack::Int(86_400), Pack::Int(42)]).unwrap();
    assert_eq!(
        v,
        Value::Temporal(Temporal::LocalDateTime(dt("1970-01-02T00:00:00.000000042")))
    );
}

#[test]
fn duration_decodes_worked_example() {
    let v = decode(
        0x45,
        vec![Pack::Int(1), Pack::Int(2), Pack::Int(3), Pack::Int(4)],
    )
    .unwrap();
    match temporal(&v) {
        Temporal::Duration {
            months,
            days,
            nanos,
        } => {
            assert_eq!(*months, 1);
            assert_eq!(*days, 2);
            assert_eq!(*nanos, 3_000_000_004);
        }
        other => panic!("expected Duration, got {other:?}"),
    }
    assert_eq!(temporal(&v).iso_string(), "P1M2DT3.000000004S");
}

#[test]
fn point_2d_decodes_worked_example() {
    let v = decode(
        0x58,
        vec![Pack::Int(7203), Pack::Float(1.0), Pack::Float(2.0)],
    )
    .unwrap();
    assert_eq!(
        v,
        Value::Point(PointValue {
            srid: 7203,
            x: 1.0,
            y: 2.0,
            z: None,
        })
    );
}

#[test]
fn point_3d_decodes_worked_example() {
    let v = decode(
        0x59,
        vec![
            Pack::Int(9157),
            Pack::Float(1.0),
            Pack::Float(2.0),
            Pack::Float(3.0),
        ],
    )
    .unwrap();
    assert_eq!(
        v,
        Value::Point(PointValue {
            srid: 9157,
            x: 1.0,
            y: 2.0,
            z: Some(3.0),
        })
    );
}

#[test]
fn all_four_srids_decode_in_their_dimension() {
    for srid in [4326, 7203] {
        let v = decode(
            0x58,
            vec![Pack::Int(srid), Pack::Float(0.0), Pack::Float(0.0)],
        )
        .unwrap();
        assert_eq!(point(&v).srid, srid as i32);
        assert_eq!(point(&v).z, None);
    }
    for srid in [4979, 9157] {
        let v = decode(
            0x59,
            vec![
                Pack::Int(srid),
                Pack::Float(0.0),
                Pack::Float(0.0),
                Pack::Float(0.0),
            ],
        )
        .unwrap();
        assert_eq!(point(&v).srid, srid as i32);
        assert_eq!(point(&v).z, Some(0.0));
    }
}

#[test]
fn non_finite_coordinates_are_stored_verbatim() {
    // ADR-0168: NaN / infinity survive the decode as Float payloads.
    let v = decode(
        0x58,
        vec![
            Pack::Int(7203),
            Pack::Float(f64::NAN),
            Pack::Float(f64::INFINITY),
        ],
    )
    .unwrap();
    assert!(point(&v).x.is_nan());
    assert_eq!(point(&v).y, f64::INFINITY);
}

#[test]
fn duration_nanos_do_not_overflow() {
    let v = decode(
        0x45,
        vec![
            Pack::Int(0),
            Pack::Int(0),
            Pack::Int(i64::MAX),
            Pack::Int(i64::MAX),
        ],
    )
    .unwrap();
    match temporal(&v) {
        Temporal::Duration { nanos, .. } => {
            assert_eq!(
                *nanos,
                i128::from(i64::MAX) * 1_000_000_000 + i128::from(i64::MAX)
            );
        }
        other => panic!("expected Duration, got {other:?}"),
    }
}

// --- DST resolution (ADR-0177 item 4) -------------------------------------

#[test]
fn zone_id_dst_gap_is_rejected() {
    // 2021-03-28T02:30 wall does not exist in Europe/Stockholm.
    let err = decode(
        0x66,
        vec![
            Pack::Int(1_616_898_600),
            Pack::Int(0),
            Pack::String("Europe/Stockholm".into()),
        ],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: ambiguous local time \
         2021-03-28T02:30:00 in timezone 'Europe/Stockholm'"
    );
}

#[test]
fn zone_id_dst_overlap_takes_earliest_offset() {
    // 2021-10-31T02:30 wall occurs twice; the earliest (pre-transition,
    // +02:00 CEST) occurrence is the engine's rule.
    let v = decode(
        0x66,
        vec![
            Pack::Int(1_635_647_400),
            Pack::Int(0),
            Pack::String("Europe/Stockholm".into()),
        ],
    )
    .unwrap();
    match temporal(&v) {
        Temporal::DateTime { utc, offset, zone } => {
            assert_eq!(*utc, dt("2021-10-31T00:30:00"));
            assert_eq!(*offset, 7200);
            assert_eq!(zone.as_deref(), Some("Europe/Stockholm"));
        }
        other => panic!("expected DateTime, got {other:?}"),
    }
    // The instant, offset, and zone are the contract; `iso_string` renders a
    // zero-seconds time without the `:00` (engine `format_time`), so the
    // canonical render is `02:30`, not the ADR prose's illustrative
    // `02:30:00`.
    assert_eq!(
        temporal(&v).iso_string(),
        "2021-10-31T02:30+02:00[Europe/Stockholm]"
    );
}

// --- Rejections (ADR-0177 item 5) -----------------------------------------

#[test]
fn wrong_field_count_is_rejected() {
    let err = decode(0x46, vec![Pack::Int(0), Pack::Int(0)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: Struct(0x46) has 2 fields, expected 3"
    );
}

#[test]
fn wrong_field_type_is_rejected() {
    let err = decode(0x44, vec![Pack::String("0".into())]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 0 of Struct(0x44) must be Int, got String"
    );
    let err = decode(0x58, vec![Pack::Int(7203), Pack::Int(1), Pack::Float(2.0)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 1 of Struct(0x58) must be Float, got Int"
    );
    let err = decode(0x66, vec![Pack::Int(0), Pack::Int(0), Pack::Int(1)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 2 of Struct(0x66) must be String, got Int"
    );
}

#[test]
fn subsecond_out_of_range_is_rejected() {
    let err = decode(
        0x46,
        vec![Pack::Int(0), Pack::Int(1_000_000_000), Pack::Int(0)],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 1 of Struct(0x46) is out of range: 1000000000"
    );
}

#[test]
fn time_of_day_out_of_range_is_rejected() {
    let err = decode(0x74, vec![Pack::Int(86_400_000_000_000)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 0 of Struct(0x74) is out of range: 86400000000000"
    );
    let err = decode(0x54, vec![Pack::Int(-1), Pack::Int(0)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 0 of Struct(0x54) is out of range: -1"
    );
}

#[test]
fn offset_that_does_not_fit_i32_is_rejected() {
    let err = decode(0x54, vec![Pack::Int(0), Pack::Int(i64::from(i32::MAX) + 1)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 1 of Struct(0x54) is out of range: 2147483648"
    );
}

#[test]
fn unknown_timezone_is_rejected() {
    let err = decode(
        0x66,
        vec![
            Pack::Int(0),
            Pack::Int(0),
            Pack::String("Mars/Olympus".into()),
        ],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: unknown timezone 'Mars/Olympus'"
    );
    // "Z" and fixed-offset spellings are not IANA zones (ADR-0177 item 4).
    let err = decode(
        0x66,
        vec![Pack::Int(0), Pack::Int(0), Pack::String("Z".into())],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: unknown timezone 'Z'"
    );
}

#[test]
fn unknown_srid_is_rejected() {
    let err = decode(
        0x58,
        vec![Pack::Int(1234), Pack::Float(1.0), Pack::Float(2.0)],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: unknown srid 1234: \
         expected one of 4326, 4979, 7203, 9157"
    );
}

#[test]
fn srid_dimension_mismatch_is_rejected() {
    let err = decode(
        0x58,
        vec![Pack::Int(9157), Pack::Float(1.0), Pack::Float(2.0)],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: srid 9157 does not match the \
         coordinate dimension of Struct(0x58)"
    );
    let err = decode(
        0x59,
        vec![
            Pack::Int(4326),
            Pack::Float(1.0),
            Pack::Float(2.0),
            Pack::Float(3.0),
        ],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: srid 4326 does not match the \
         coordinate dimension of Struct(0x59)"
    );
}

#[test]
fn unsupported_tags_are_rejected() {
    // The caller rejects these before the decoder; it still stays total.
    let err = decode(
        0x4E,
        vec![Pack::Int(1), Pack::List(vec![]), Pack::Map(vec![])],
    )
    .unwrap_err();
    assert_eq!(err, "unsupported parameter value for $p: Struct(0x4E)");
    let err = decode(0x49, vec![Pack::Int(0), Pack::Int(0)]).unwrap_err();
    assert_eq!(err, "unsupported parameter value for $p: Struct(0x49)");
    let err = decode(0x69, vec![Pack::Int(0), Pack::Int(0)]).unwrap_err();
    assert_eq!(err, "unsupported parameter value for $p: Struct(0x69)");
}

// --- The utc patch (ADR-0329) ---------------------------------------------

#[test]
fn utc_patch_decodes_0x49_from_the_utc_instant() {
    // F-57's value: 2024-01-02T03:04:05.123456789+01:00 is UTC 02:04:05.
    let v = decode_utc(
        0x49,
        vec![Pack::Int(1704161045), Pack::Int(123456789), Pack::Int(3600)],
    )
    .unwrap();
    assert_eq!(
        temporal(&v),
        &Temporal::DateTime {
            utc: dt("2024-01-02T02:04:05.123456789"),
            offset: 3600,
            zone: None,
        }
    );
    assert_eq!(
        temporal(&v).iso_string(),
        "2024-01-02T03:04:05.123456789+01:00"
    );
}

#[test]
fn utc_patch_decodes_0x69_with_the_offset_at_that_instant() {
    // 2021-10-31T00:30Z is 02:30 CEST (+02:00), inside the wall-clock DST
    // overlap that 0x66 can only resolve to the earliest offset; one hour
    // later, 01:30Z, is 02:30 CET (+01:00). The UTC instant tells them apart.
    let first = decode_utc(
        0x69,
        vec![
            Pack::Int(1635640200),
            Pack::Int(0),
            Pack::String("Europe/Stockholm".into()),
        ],
    )
    .unwrap();
    assert_eq!(
        temporal(&first).iso_string(),
        "2021-10-31T02:30+02:00[Europe/Stockholm]"
    );
    let second = decode_utc(
        0x69,
        vec![
            Pack::Int(1635643800),
            Pack::Int(0),
            Pack::String("Europe/Stockholm".into()),
        ],
    )
    .unwrap();
    assert_eq!(
        temporal(&second),
        &Temporal::DateTime {
            utc: dt("2021-10-31T01:30:00"),
            offset: 3600,
            zone: Some("Europe/Stockholm".into()),
        }
    );
}

#[test]
fn utc_patch_rejects_unknown_zones_and_legacy_datetimes() {
    let err = decode_utc(
        0x69,
        vec![Pack::Int(0), Pack::Int(0), Pack::String("Mars/Base".into())],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: unknown timezone 'Mars/Base'"
    );
    // Under the patch the legacy pair is an unsupported struct, and the
    // non-DateTime tags are unchanged.
    let err = decode_utc(0x46, vec![Pack::Int(0), Pack::Int(0), Pack::Int(0)]).unwrap_err();
    assert_eq!(err, "unsupported parameter value for $p: Struct(0x46)");
    let err = decode_utc(
        0x66,
        vec![
            Pack::Int(0),
            Pack::Int(0),
            Pack::String("Europe/Paris".into()),
        ],
    )
    .unwrap_err();
    assert_eq!(err, "unsupported parameter value for $p: Struct(0x66)");
    assert_eq!(
        decode_utc(0x44, vec![Pack::Int(0)]).unwrap(),
        decode(0x44, vec![Pack::Int(0)]).unwrap()
    );
    // The UTC tags keep the field contract.
    let err = decode_utc(
        0x49,
        vec![Pack::Int(0), Pack::Int(1_000_000_000), Pack::Int(0)],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 1 of Struct(0x49) is out of range: 1000000000"
    );
}

// --- Bolt 5.4 (ADR-0484 decision 7) ----------------------------------------

#[test]
fn at_5_4_the_utc_pair_decodes_with_no_patch() {
    // The fixed forms are unconditional on a 5.x connection, parameters
    // and results alike: the same decoders the utc patch selects at 4.4,
    // with nothing negotiated.
    let v = decode_5_4(
        0x49,
        vec![Pack::Int(1704161045), Pack::Int(123456789), Pack::Int(3600)],
    )
    .unwrap();
    assert_eq!(
        temporal(&v).iso_string(),
        "2024-01-02T03:04:05.123456789+01:00"
    );
    let v = decode_5_4(
        0x69,
        vec![
            Pack::Int(1635640200),
            Pack::Int(0),
            Pack::String("Europe/Stockholm".into()),
        ],
    )
    .unwrap();
    assert_eq!(
        temporal(&v).iso_string(),
        "2021-10-31T02:30+02:00[Europe/Stockholm]"
    );
}

#[test]
fn at_5_4_the_legacy_datetime_pair_is_rejected() {
    // The legacy 0x46/0x66 were removed after 4.4: on a 5.x connection
    // they are unsupported parameters.
    let err = decode_5_4(0x46, vec![Pack::Int(0), Pack::Int(0), Pack::Int(0)]).unwrap_err();
    assert_eq!(err, "unsupported parameter value for $p: Struct(0x46)");
    let err = decode_5_4(
        0x66,
        vec![
            Pack::Int(0),
            Pack::Int(0),
            Pack::String("Europe/Paris".into()),
        ],
    )
    .unwrap_err();
    assert_eq!(err, "unsupported parameter value for $p: Struct(0x66)");
    // The non-DateTime tags are unchanged at 5.4.
    assert_eq!(
        decode_5_4(0x44, vec![Pack::Int(0)]).unwrap(),
        decode(0x44, vec![Pack::Int(0)]).unwrap()
    );
}

// --- Bytes (ADR-0328) -----------------------------------------------------

#[test]
fn bytes_decode_to_the_engine_byte_array_at_any_depth() {
    assert_eq!(
        Pack::Bytes(vec![0x00, 0x01, 0xFF]).to_mdb(),
        Ok(Value::Bytes(vec![0x00, 0x01, 0xFF].into()))
    );
    assert_eq!(
        Pack::Bytes(vec![]).to_mdb(),
        Ok(Value::Bytes(Vec::new().into()))
    );
    let nested = Pack::Map(vec![(
        Pack::String("xs".into()),
        Pack::List(vec![Pack::Bytes(vec![7])]),
    )]);
    let Value::Map(m) = nested.to_mdb().unwrap() else {
        panic!("a map decodes to a map");
    };
    assert_eq!(
        m["xs"],
        Value::List(vec![Value::Bytes(vec![7].into())].into())
    );
}

#[test]
fn to_mdb_is_fallible_and_path_qualified() {
    // Only `Pack::Null` maps to `Value::Null`; no catch-all arm hides an
    // unsupported value behind a silent null (ADR-0177).
    assert_eq!(Pack::Null.to_mdb(), Ok(Value::Null));
    assert_eq!(Pack::Bool(true).to_mdb(), Ok(Value::Bool(true)));
    assert_eq!(Pack::Int(7).to_mdb(), Ok(Value::Int(7)));
    assert_eq!(Pack::Float(1.5).to_mdb(), Ok(Value::Float(1.5)));
    assert_eq!(
        Pack::String("s".into()).to_mdb(),
        Ok(Value::String("s".into()))
    );

    assert_eq!(
        Pack::Struct {
            tag: 0x4E,
            fields: vec![],
        }
        .to_mdb(),
        Err("unsupported parameter value for $: Struct(0x4E)".into())
    );
    assert_eq!(
        Pack::Map(vec![(Pack::Int(1), Pack::Int(2))]).to_mdb(),
        Err("unsupported parameter map key for $: Int (map keys must be strings)".into())
    );

    // The path names the position of the offending value: `to_mdb`'s root is
    // the value itself (`$`), so a map entry extends it with `.key` and a
    // list element with `[i]` — `$.p[0]` for a Node struct nested in a list
    // inside a map. The wire path seeds each top-level parameter as `$name`
    // through `to_mdb_at`, so the same value is `$p[0]` there (see
    // `params_to_map`).
    let node_in_list = Pack::List(vec![Pack::Struct {
        tag: 0x4E,
        fields: vec![],
    }]);
    let nested = Pack::Map(vec![(Pack::String("p".into()), node_in_list.clone())]);
    assert_eq!(
        nested.to_mdb(),
        Err("unsupported parameter value for $.p[0]: Struct(0x4E)".into())
    );
    let mut path = ParamPath::param("p");
    assert_eq!(
        node_in_list.to_mdb_at(&mut path, Dialect::plain_44()),
        Err("unsupported parameter value for $p[0]: Struct(0x4E)".into())
    );

    // Deeper paths extend with `.key` and `[i]` at every level.
    let deep_value = Pack::Map(vec![(
        Pack::String("xs".into()),
        Pack::List(vec![Pack::Struct {
            tag: 0x49,
            fields: vec![],
        }]),
    )]);
    let deep = Pack::Map(vec![(Pack::String("p".into()), deep_value.clone())]);
    assert_eq!(
        deep.to_mdb(),
        Err("unsupported parameter value for $.p.xs[0]: Struct(0x49)".into())
    );
    let mut path = ParamPath::param("p");
    assert_eq!(
        deep_value.to_mdb_at(&mut path, Dialect::plain_44()),
        Err("unsupported parameter value for $p.xs[0]: Struct(0x49)".into())
    );

    // A non-string key inside a nested map names the map's own path.
    let bad_key = Pack::Map(vec![(Pack::Bool(false), Pack::Null)]);
    let nested_key = Pack::Map(vec![(Pack::String("p".into()), bad_key.clone())]);
    assert_eq!(
        nested_key.to_mdb(),
        Err("unsupported parameter map key for $.p: Bool (map keys must be strings)".into())
    );
    let mut path = ParamPath::param("p");
    assert_eq!(
        bad_key.to_mdb_at(&mut path, Dialect::plain_44()),
        Err("unsupported parameter map key for $p: Bool (map keys must be strings)".into())
    );
}

// --- Checked arithmetic (no panic on wire extremes) -----------------------

#[test]
fn date_days_overflow_is_rejected() {
    let err = decode(0x44, vec![Pack::Int(i64::MAX)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 0 of Struct(0x44) is out of range: \
         9223372036854775807"
    );
}

#[test]
fn datetime_seconds_underflow_is_rejected() {
    let err = decode(0x46, vec![Pack::Int(i64::MIN), Pack::Int(0), Pack::Int(0)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 0 of Struct(0x46) is out of range: \
         -9223372036854775808"
    );
    // i64::MIN - 1 overflows the offset subtraction itself.
    let err = decode(0x46, vec![Pack::Int(i64::MIN), Pack::Int(0), Pack::Int(1)]).unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p: field 0 of Struct(0x46) is out of range: \
         -9223372036854775808"
    );
}

// --- Path threading -------------------------------------------------------

#[test]
fn messages_carry_the_caller_supplied_path() {
    let mut path = ParamPath::param("p");
    path.push(PathSeg::Key("xs"));
    path.push(PathSeg::Index(0));
    let err = struct_to_value(
        0x44,
        &[Pack::String("x".into())],
        &path,
        Dialect::plain_44(),
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid parameter structure for $p.xs[0]: field 0 of Struct(0x44) must be Int, got String"
    );
}
