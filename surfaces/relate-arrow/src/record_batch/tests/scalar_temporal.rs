use super::super::*;
use super::{make_rows, one_col};
use arrow_array::{Time64NanosecondArray, TimestampNanosecondArray};
use chrono::TimeZone;
use relate_model::value::IsoDuration;

#[test]
fn test_result_to_record_batch() {
    let cols = vec!["name".to_string(), "age".to_string(), "active".to_string()];
    let rows = make_rows();
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(batch.num_columns(), 3);
    let names = batch
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(names.value(0), "Alice");
    let ages = batch
        .column(1)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(ages.value(0), 30);
    let active = batch
        .column(2)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .unwrap();
    assert!(active.value(0));
}

#[test]
fn test_empty_result_to_record_batch() {
    let cols = vec!["a".to_string(), "b".to_string()];
    let rows: Vec<IndexMap<String, CypherValue>> = vec![];
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(batch.num_rows(), 0);
    assert_eq!(batch.num_columns(), 2);
    // Both columns default to Utf8 when there is nothing to infer from.
    assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
}

#[test]
fn null_first_column_is_typed_by_later_rows() {
    // B3: a null first value must NOT force the whole column to Utf8/None.
    let (cols, rows) = one_col(
        "x",
        vec![CypherValue::Null, CypherValue::Int(7), CypherValue::Int(9)],
    );
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(batch.schema().field(0).data_type(), &DataType::Int64);
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert!(arr.is_null(0));
    assert_eq!(arr.value(1), 7);
    assert_eq!(arr.value(2), 9);
}

#[test]
fn heterogeneous_int_float_promotes_and_keeps_all_values() {
    // B3: Int then Float -> Float64 with EVERY present value retained.
    let (cols, rows) = one_col(
        "x",
        vec![
            CypherValue::Int(1),
            CypherValue::Float(2.5),
            CypherValue::Int(3),
        ],
    );
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(batch.schema().field(0).data_type(), &DataType::Float64);
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    assert_eq!(arr.null_count(), 0);
    assert_eq!(arr.value(0), 1.0);
    assert_eq!(arr.value(1), 2.5);
    assert_eq!(arr.value(2), 3.0);
}

#[test]
fn inexact_int_float_uses_typed_json_without_losing_integer_bits() {
    for integer in [9_007_199_254_740_993, -9_007_199_254_740_993, i64::MAX] {
        for values in [
            vec![CypherValue::Int(integer), CypherValue::Float(2.5)],
            vec![CypherValue::Float(2.5), CypherValue::Int(integer)],
        ] {
            let (cols, mut rows) = one_col("amount", values);
            rows.insert(1, IndexMap::new()); // An absent row also gets a null cell.
            assert_eq!(
                schema::infer_column_type("amount", &rows),
                ColType::TypedJson
            );
            for batches in [
                vec![result_to_record_batch(&cols, &rows).unwrap()],
                result_to_record_batches(&cols, &rows, 1).unwrap(),
            ] {
                for batch in &batches {
                    assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
                    assert_eq!(
                        batch.schema().field(0).metadata()[RELATE_ENCODING_KEY],
                        ENCODING_TYPED_JSON_V1
                    );
                    assert_eq!(batch.schema(), batches[0].schema());
                }
                let decoded = batches
                    .iter()
                    .flat_map(|batch| crate::from_arrow::record_batch_to_rows(batch).unwrap().1)
                    .collect::<Vec<_>>();
                let expected = rows
                    .iter()
                    .map(|row| {
                        IndexMap::from([(
                            "amount".to_owned(),
                            row.get("amount").cloned().unwrap_or(CypherValue::Null),
                        )])
                    })
                    .collect::<Vec<_>>();
                assert_eq!(decoded, expected);
            }
        }
    }
}

#[test]
fn exact_large_int_float_mixes_stay_native_float64() {
    for integer in [i64::MIN, 9_007_199_254_740_994] {
        for values in [
            vec![CypherValue::Int(integer), CypherValue::Float(2.5)],
            vec![CypherValue::Float(2.5), CypherValue::Int(integer)],
        ] {
            let (cols, rows) = one_col("amount", values);
            assert_eq!(schema::infer_column_type("amount", &rows), ColType::Float);
            for batch in std::iter::once(result_to_record_batch(&cols, &rows).unwrap())
                .chain(result_to_record_batches(&cols, &rows, 1).unwrap())
            {
                assert_eq!(batch.schema().field(0).data_type(), &DataType::Float64);
                assert!(!batch
                    .schema()
                    .field(0)
                    .metadata()
                    .contains_key(RELATE_ENCODING_KEY));
            }
        }
    }
    let (cols, rows) = one_col("amount", vec![CypherValue::Int(i64::MAX)]);
    assert_eq!(
        result_to_record_batch(&cols, &rows)
            .unwrap()
            .schema()
            .field(0)
            .data_type(),
        &DataType::Int64
    );
}

#[test]
fn date_maps_to_date32() {
    let (cols, rows) = one_col(
        "d",
        vec![CypherValue::Date(
            NaiveDate::from_ymd_opt(2024, 3, 14).unwrap(),
        )],
    );
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(batch.schema().field(0).data_type(), &DataType::Date32);
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<Date32Array>()
        .unwrap();
    // 2024-03-14 is 19796 days after the Unix epoch.
    assert_eq!(arr.value(0), 19796);
}

#[test]
fn time_maps_to_time64_not_json_text() {
    let (cols, rows) = one_col(
        "t",
        vec![CypherValue::Time(
            chrono::NaiveTime::from_hms_opt(1, 0, 0).unwrap(),
            None,
        )],
    );
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert!(matches!(
        batch.schema().field(0).data_type(),
        DataType::Time64(_)
    ));
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<Time64NanosecondArray>()
        .unwrap();
    assert_eq!(arr.value(0), 3600 * NANOS_PER_SEC);
    assert_eq!(
        crate::from_arrow::record_batch_to_rows(&batch).unwrap().1[0]["t"],
        rows[0]["t"]
    );
}

#[test]
fn zoned_time_and_unzoned_time_use_lossless_typed_json_in_single_and_chunked_batches() {
    let t = chrono::NaiveTime::from_hms_nano_opt(12, 34, 56, 123_456_789).unwrap();
    let zoned = CypherValue::Time(t, Some(FixedOffset::east_opt(2 * 3600 + 30 * 60).unwrap()));
    let unzoned = CypherValue::Time(t, None);
    for values in [
        vec![zoned.clone()],
        vec![unzoned.clone(), CypherValue::Null, zoned.clone()],
        vec![zoned.clone(), unzoned.clone(), CypherValue::Int(7)],
        vec![CypherValue::Map(IndexMap::from([(
            "inner".into(),
            CypherValue::List(vec![zoned.clone(), unzoned.clone()]),
        )]))],
    ] {
        let (cols, rows) = one_col("t", values);
        let expected_field =
            Field::new("t", DataType::Utf8, true).with_metadata(HashMap::from([(
                RELATE_ENCODING_KEY.to_owned(),
                ENCODING_TYPED_JSON_V1.to_owned(),
            )]));
        let single = result_to_record_batch(&cols, &rows).unwrap();
        assert_eq!(single.schema().field(0), &expected_field);
        let array = single
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        for (row, original) in rows.iter().enumerate() {
            if original["t"] == zoned {
                let envelope: serde_json::Value = serde_json::from_str(array.value(row)).unwrap();
                assert_eq!(envelope["type"], "TIME");
                assert_eq!(envelope["value"]["offset"], "+02:30");
            }
        }
        let decoded = crate::from_arrow::record_batch_to_rows(&single).unwrap().1;
        for (original, actual) in rows.iter().zip(&decoded) {
            assert_eq!(actual["t"], original["t"]);
        }
        let chunks = result_to_record_batches(&cols, &rows, 1).unwrap();
        for (original, chunk) in rows.iter().zip(&chunks) {
            assert_eq!(chunk.schema().field(0), &expected_field);
            assert_eq!(
                crate::from_arrow::record_batch_to_rows(chunk).unwrap().1[0]["t"],
                original["t"]
            );
        }
    }
}

#[test]
fn subminute_zoned_time_errors_through_model_payload_writer() {
    let t = chrono::NaiveTime::from_hms_opt(12, 34, 56).unwrap();
    for seconds in [1, -1] {
        let bad = CypherValue::Time(t, Some(FixedOffset::east_opt(seconds).unwrap()));
        for (value, path) in [
            (bad.clone(), "$"),
            (
                CypherValue::Map(IndexMap::from([(
                    "nested".into(),
                    CypherValue::List(vec![CypherValue::Time(t, None), bad]),
                )])),
                "$.nested[1]",
            ),
        ] {
            let (cols, rows) = one_col("t", vec![CypherValue::Time(t, None), value]);
            for result in [
                result_to_record_batch(&cols, &rows).map(|_| ()),
                result_to_record_batches(&cols, &rows, 1).map(|_| ()),
            ] {
                let err = result.unwrap_err();
                assert!(err.to_string().contains("column 't' row 1"), "{err}");
                assert!(err.to_string().contains(path), "{err}");
                assert!(err.to_string().contains("offset"), "{err}");
            }
        }
    }
}

#[test]
fn datetime_fallback_rejects_subminute_offsets_in_any_row_and_chunk() {
    let at = NaiveDate::from_ymd_opt(2024, 3, 14)
        .unwrap()
        .and_hms_nano_opt(12, 30, 45, 123_456_789)
        .unwrap();
    let valid = FixedOffset::east_opt(2 * 3600 + 30 * 60)
        .unwrap()
        .from_local_datetime(&at)
        .unwrap();
    for seconds in [1, -1] {
        let bad = FixedOffset::east_opt(seconds)
            .unwrap()
            .from_local_datetime(&at)
            .unwrap();
        for value in [
            CypherValue::DateTime(bad, None),
            CypherValue::DateTime(bad, Some("Europe/Stockholm".into())),
        ] {
            for first in [
                CypherValue::DateTime(valid, None),
                CypherValue::DateTime(valid, Some("Europe/Stockholm".into())),
            ] {
                for (values, bad_row) in [
                    (vec![value.clone(), first.clone()], 0),
                    (vec![first, CypherValue::Null, value.clone()], 2),
                ] {
                    let (cols, rows) = one_col("dt", values);
                    assert_eq!(schema::infer_column_type("dt", &rows), ColType::TypedJson);
                    for result in [
                        result_to_record_batch(&cols, &rows).map(|_| ()),
                        result_to_record_batches(&cols, &rows, 1).map(|_| ()),
                    ] {
                        let err = result.unwrap_err();
                        assert!(matches!(err, ArrowError::InvalidArgumentError(_)));
                        assert!(
                            err.to_string()
                                .contains(&format!("column 'dt' row {bad_row}")),
                            "{err}"
                        );
                        assert!(err.to_string().contains("offset"), "{err}");
                    }
                }
            }
        }
    }
}

#[test]
fn out_of_range_datetime_uses_model_offset_guard_in_typed_json() {
    let at = NaiveDate::from_ymd_opt(1500, 3, 14)
        .unwrap()
        .and_hms_opt(12, 30, 45)
        .unwrap();
    let bad = FixedOffset::east_opt(1)
        .unwrap()
        .from_local_datetime(&at)
        .unwrap();
    let (cols, rows) = one_col("dt", vec![CypherValue::DateTime(bad, None)]);
    assert_eq!(schema::infer_column_type("dt", &rows), ColType::TypedJson);
    for result in [
        result_to_record_batch(&cols, &rows).map(|_| ()),
        result_to_record_batches(&cols, &rows, 1).map(|_| ()),
    ] {
        let err = result.unwrap_err();
        assert!(err.to_string().contains("column 'dt' row 0"), "{err}");
        assert!(err.to_string().contains("offset"), "{err}");
    }
}

#[test]
fn native_datetime_fixed_offset_roundtrips_offset_and_utc_instant() {
    let off = FixedOffset::east_opt(2 * 3600 + 30 * 60).unwrap();
    let instant = off
        .with_ymd_and_hms(2024, 3, 14, 12, 30, 45)
        .unwrap()
        .with_nanosecond(123_456_789)
        .unwrap();
    let later = instant + chrono::Duration::hours(12);
    let (cols, rows) = one_col(
        "dt",
        vec![
            CypherValue::Null,
            CypherValue::DateTime(instant, None),
            CypherValue::DateTime(later, None),
        ],
    );
    assert_eq!(schema::infer_column_type("dt", &rows), ColType::DateTime);
    assert_eq!(
        plan_columns(&cols, &rows).unwrap()[0].tz.as_deref(),
        Some("+02:30")
    );
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(
        batch.schema().field(0).data_type(),
        &DataType::Timestamp(arrow_schema::TimeUnit::Nanosecond, Some("+02:30".into()))
    );
    for batches in [
        vec![batch],
        result_to_record_batches(&cols, &rows, 1).unwrap(),
    ] {
        let decoded = batches
            .iter()
            .flat_map(|batch| {
                assert_eq!(
                    batch.schema().field(0).data_type(),
                    &DataType::Timestamp(arrow_schema::TimeUnit::Nanosecond, Some("+02:30".into()))
                );
                crate::from_arrow::record_batch_to_rows(batch).unwrap().1
            })
            .collect::<Vec<_>>();
        assert_eq!(decoded.len(), rows.len());
        for (expected, actual) in rows.iter().zip(&decoded) {
            assert_datetime_value(&expected["dt"], &actual["dt"]);
        }
    }
}

fn assert_datetime_value(expected: &CypherValue, actual: &CypherValue) {
    match (expected, actual) {
        (CypherValue::Null, CypherValue::Null) => {}
        (CypherValue::DateTime(expected_dt, expected_name), CypherValue::DateTime(dt, name)) => {
            assert_eq!(dt.offset(), expected_dt.offset());
            assert_eq!(dt.naive_local(), expected_dt.naive_local());
            assert_eq!(dt.naive_utc(), expected_dt.naive_utc());
            assert_eq!(dt.timestamp_nanos_opt(), expected_dt.timestamp_nanos_opt());
            assert_eq!(name, expected_name);
        }
        _ => panic!("expected {expected:?}, got {actual:?}"),
    }
}

#[test]
fn heterogeneous_and_named_datetime_columns_preserve_each_row_in_single_and_chunked_batches() {
    let plus = FixedOffset::east_opt(2 * 3600 + 30 * 60).unwrap();
    let minus = FixedOffset::west_opt(5 * 3600).unwrap();
    let early = plus.with_ymd_and_hms(2024, 3, 14, 12, 30, 45).unwrap();
    let later = minus.with_ymd_and_hms(2024, 3, 15, 8, 9, 10).unwrap();
    let cases = [
        vec![
            CypherValue::DateTime(early, None),
            CypherValue::DateTime(later, None),
        ],
        vec![CypherValue::DateTime(
            early,
            Some("Europe/Stockholm".into()),
        )],
        vec![
            CypherValue::DateTime(early, None),
            CypherValue::Null,
            CypherValue::DateTime(later, Some("America/New_York".into())),
        ],
    ];
    for values in cases {
        let (cols, rows) = one_col("dt", values);
        assert_eq!(schema::infer_column_type("dt", &rows), ColType::TypedJson);
        for batches in [
            vec![result_to_record_batch(&cols, &rows).unwrap()],
            result_to_record_batches(&cols, &rows, 1).unwrap(),
        ] {
            let mut first_row = 0;
            for batch in &batches {
                assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
                assert_eq!(
                    batch.schema().field(0).metadata()[RELATE_ENCODING_KEY],
                    ENCODING_TYPED_JSON_V1
                );
                let strings = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap();
                for row in 0..batch.num_rows() {
                    if strings.is_null(row) {
                        continue;
                    }
                    let payload: serde_json::Value =
                        serde_json::from_str(strings.value(row)).unwrap();
                    let original = &rows[first_row + row]["dt"];
                    if let CypherValue::DateTime(dt, name) = original {
                        assert_eq!(payload["type"], "DATE_TIME");
                        assert_eq!(payload["value"]["value"], dt.to_rfc3339());
                        assert_eq!(
                            payload["value"].get("timezone").and_then(|v| v.as_str()),
                            name.as_deref()
                        );
                    }
                }
                first_row += batch.num_rows();
            }
            let decoded = batches
                .iter()
                .flat_map(|batch| crate::from_arrow::record_batch_to_rows(batch).unwrap().1)
                .collect::<Vec<_>>();
            assert_eq!(decoded.len(), rows.len());
            for (expected, actual) in rows.iter().zip(&decoded) {
                assert_datetime_value(&expected["dt"], &actual["dt"]);
            }
        }
    }
}

#[test]
fn localdatetime_maps_to_timestamp() {
    let ndt = NaiveDate::from_ymd_opt(2024, 3, 14)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let (cols, rows) = one_col("dt", vec![CypherValue::LocalDateTime(ndt)]);
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert!(matches!(
        batch.schema().field(0).data_type(),
        DataType::Timestamp(_, None)
    ));
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<TimestampNanosecondArray>()
        .unwrap();
    assert_eq!(arr.value(0), ndt.and_utc().timestamp_nanos_opt().unwrap());
}

#[test]
fn native_timestamps_preserve_historical_instants_and_reject_out_of_range_plans() {
    let historical = chrono::DateTime::parse_from_rfc3339("1800-01-01T12:34:56+02:00").unwrap();
    let ancient = chrono::DateTime::parse_from_rfc3339("1500-01-01T12:34:56+02:00").unwrap();
    for (kind, valid, invalid) in [
        (
            ColType::DateTime,
            CypherValue::DateTime(historical, None),
            CypherValue::DateTime(ancient, None),
        ),
        (
            ColType::LocalDateTime,
            CypherValue::LocalDateTime(historical.naive_local()),
            CypherValue::LocalDateTime(ancient.naive_local()),
        ),
    ] {
        let (cols, rows) = one_col("dt", vec![CypherValue::Null, valid.clone()]);
        let batch = result_to_record_batch(&cols, &rows).unwrap();
        let arr = batch
            .column(0)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        assert!(arr.is_null(0));
        let expected = match &valid {
            CypherValue::DateTime(dt, _) => dt.timestamp_nanos_opt().unwrap(),
            CypherValue::LocalDateTime(dt) => dt.and_utc().timestamp_nanos_opt().unwrap(),
            _ => unreachable!(),
        };
        assert!(expected < 0);
        assert_eq!(arr.value(1), expected);

        let (cols, rows) = one_col("dt", vec![invalid.clone()]);
        let batch = result_to_record_batch(&cols, &rows).unwrap();
        assert_eq!(
            batch
                .schema()
                .field(0)
                .metadata()
                .get(RELATE_ENCODING_KEY)
                .map(String::as_str),
            Some(ENCODING_TYPED_JSON_V1)
        );
        let json = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(typed_json::decode(json.value(0)).unwrap(), invalid);
        let err = build_column(
            &ColPlan {
                name: cols[0].clone(),
                kind,
                srid: None,
                tz: None,
            },
            &rows,
            0,
        )
        .unwrap_err();
        assert!(matches!(err, ArrowError::InvalidArgumentError(_)));
        assert!(err.to_string().contains("column 'dt' row 0"), "{err}");
        assert!(err.to_string().contains("timestamp range"), "{err}");
    }
}

#[test]
fn duration_maps_to_interval() {
    let (cols, rows) = one_col(
        "d",
        vec![CypherValue::Duration(IsoDuration::new(14, 3, 3661, 500))],
    );
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert!(matches!(
        batch.schema().field(0).data_type(),
        DataType::Interval(_)
    ));
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<IntervalMonthDayNanoArray>()
        .unwrap();
    let v = arr.value(0);
    assert_eq!(v.months, 14);
    assert_eq!(v.days, 3);
    assert_eq!(v.nanoseconds, 3661 * NANOS_PER_SEC + 500);
}

#[test]
fn native_duration_keeps_signed_bounds_and_rejects_unrepresentable_plans() {
    let min = IsoDuration::new(
        i32::MIN as i64,
        i32::MAX as i64,
        -9_223_372_036,
        -854_775_808,
    );
    let max = IsoDuration::new(i32::MAX as i64, i32::MIN as i64, 9_223_372_036, 854_775_807);
    let (cols, rows) = one_col(
        "dur",
        vec![
            CypherValue::Null,
            CypherValue::Duration(min.clone()),
            CypherValue::Duration(max),
        ],
    );
    let arr = result_to_record_batch(&cols, &rows).unwrap();
    let values = arr
        .column(0)
        .as_any()
        .downcast_ref::<IntervalMonthDayNanoArray>()
        .unwrap();
    assert!(values.is_null(0));
    assert_eq!(
        values.value(1),
        IntervalMonthDayNano::new(i32::MIN, i32::MAX, i64::MIN)
    );
    assert_eq!(
        values.value(2),
        IntervalMonthDayNano::new(i32::MAX, i32::MIN, i64::MAX)
    );

    for (invalid, cause) in [
        (IsoDuration::new(i32::MAX as i64 + 1, 0, 0, 0), "months"),
        (IsoDuration::new(0, i32::MIN as i64 - 1, 0, 0), "days"),
        (IsoDuration::new(0, 0, i64::MAX, 0), "nanoseconds"),
        (
            IsoDuration::new(0, 0, 9_223_372_036, 854_775_808),
            "nanoseconds",
        ),
        (
            IsoDuration::new(0, 0, -9_223_372_036, -854_775_809),
            "nanoseconds",
        ),
    ] {
        let (cols, rows) = one_col("dur", vec![CypherValue::Duration(invalid.clone())]);
        let batch = result_to_record_batch(&cols, &rows).unwrap();
        assert_eq!(
            batch
                .schema()
                .field(0)
                .metadata()
                .get(RELATE_ENCODING_KEY)
                .map(String::as_str),
            Some(ENCODING_TYPED_JSON_V1)
        );
        let json = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(
            typed_json::decode(json.value(0)).unwrap(),
            CypherValue::Duration(invalid)
        );

        let err = build_column(
            &ColPlan {
                name: cols[0].clone(),
                kind: ColType::Duration,
                srid: None,
                tz: None,
            },
            &rows,
            0,
        )
        .unwrap_err();
        assert!(matches!(err, ArrowError::InvalidArgumentError(_)));
        assert!(err.to_string().contains("column 'dur' row 0"), "{err}");
        assert!(err.to_string().contains(cause), "{err}");
    }
}

#[test]
fn chunked_batches_share_schema() {
    let (cols, rows) = one_col(
        "x",
        vec![
            CypherValue::Int(1),
            CypherValue::Int(2),
            CypherValue::Int(3),
            CypherValue::Int(4),
            CypherValue::Int(5),
        ],
    );
    let batches = result_to_record_batches(&cols, &rows, 2).unwrap();
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].num_rows(), 2);
    assert_eq!(batches[2].num_rows(), 1);
    let s0 = batches[0].schema();
    for b in &batches {
        assert_eq!(b.schema(), s0);
        assert_eq!(b.schema().field(0).data_type(), &DataType::Int64);
    }
}

#[test]
fn test_result_to_string_batch() {
    let cols = vec!["name".to_string(), "age".to_string()];
    let rows = make_rows();
    let batch = result_to_string_batch(&cols, &rows).unwrap();
    assert_eq!(batch.num_rows(), 2);
    let age_col = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(age_col.value(0), "30");
}
