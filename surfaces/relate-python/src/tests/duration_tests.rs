//! The duration crossing (ADR-0502 §1): every duration result is the
//! lossless `relate.Duration`, `timedelta` parameters still bind, and
//! `Duration.to_timedelta()` is the checked projection (LABS-712's range
//! edges).

use super::*;

#[test]
fn test_timedelta_param_is_duration() {
    with_py(|py| {
        let dtmod = py.import("datetime").unwrap();
        // timedelta(days=1, seconds=30)
        let td = dtmod.call_method1("timedelta", (1, 30)).unwrap();
        let cv = py_to_cypher_value(&td).unwrap();
        match cv {
            CypherValue::Duration(d) => {
                assert_eq!(d.months, 0);
                assert_eq!(d.days, 1);
                assert_eq!(d.seconds, 30);
            }
            other => panic!("expected Duration, got {other:?}"),
        }
    });
}

/// The `timedelta` projection of a duration result (`Duration.to_timedelta`).
fn duration_timedelta<'py>(py: Python<'py>, dur: IsoDuration) -> PyResult<Bound<'py, PyAny>> {
    duration_to_py(py, &dur)
        .unwrap()
        .into_bound(py)
        .call_method0("to_timedelta")
}

/// LABS-712: `timedelta` holds at most ±999_999_999 days, so a larger
/// duration has no `timedelta` projection — a typed `ValueError`, never the
/// C constructor's raw `OverflowError`. The duration itself still crosses.
#[test]
fn test_out_of_range_duration_has_no_timedelta() {
    with_py(|py| {
        for dur in [
            IsoDuration::new(0, 1_000_000_000, 0, 0),
            // One over the bound only after the seconds normalize into days.
            IsoDuration::new(0, 999_999_999, 86_400, 0),
            IsoDuration::new(0, -1_000_000_000, 0, 0),
        ] {
            let obj = cypher_value_to_py(py, &CypherValue::Duration(dur.clone()), Namespace::Neo4j)
                .unwrap()
                .into_bound(py);
            assert_eq!(obj.str().unwrap().to_string(), dur.to_string());
            let err = obj.call_method0("to_timedelta").unwrap_err();
            assert!(err.is_instance_of::<PyValueError>(py), "{err}");
        }
        // A duration inside the range projects to a real timedelta.
        let td = duration_timedelta(py, IsoDuration::new(0, 1, 30, 0)).unwrap();
        let dtmod = py.import("datetime").unwrap();
        assert!(td
            .is_instance(&dtmod.getattr("timedelta").unwrap())
            .unwrap());
    });
}

#[test]
fn test_max_timedelta_param_roundtrips() {
    with_py(|py| {
        let dtmod = py.import("datetime").unwrap();
        let td = dtmod.getattr("timedelta").unwrap().getattr("max").unwrap();
        let cv = py_to_cypher_value(&td).unwrap();
        let back = cypher_value_to_py(py, &cv, Namespace::Neo4j)
            .unwrap()
            .into_bound(py)
            .call_method0("to_timedelta")
            .unwrap();
        assert!(back.eq(&td).unwrap(), "{back:?} != {td:?}");
    });
}

#[test]
fn test_timedelta_param_out_of_range_raises_typed_overflow() {
    with_py(|py| {
        // A stdlib `timedelta` is bounded, but a subclass can shadow `days`.
        // The parameter boundary must raise the binding's typed error instead
        // of leaking pyo3's C-width wording.
        let obj = py
            .eval(
                c"type('T', (__import__('datetime').timedelta,), {'days': property(lambda self: 2**70)})(1)",
                None,
                None,
            )
            .unwrap();
        let err = py_to_cypher_value(&obj).unwrap_err();
        assert!(err.is_instance_of::<PyOverflowError>(py), "{err}");
        assert!(err.to_string().contains("'days'"), "{err}");
    });
}

#[test]
fn test_large_in_range_duration_projects_to_timedelta() {
    with_py(|py| {
        let mut db = new_database(":memory:");
        // The engine keeps 2_400_000_000 raw seconds; CPython rejects that
        // argument unnormalized. 27_777 days plus 18:40 is within
        // `timedelta`'s range, so the projection must succeed.
        let value = query_first_py(py, &mut db, "RETURN duration('PT2400000000S') AS d", "d");
        assert_eq!(
            value.getattr("seconds").unwrap().extract::<i64>().unwrap(),
            2_400_000_000
        );
        let dtmod = py.import("datetime").unwrap();
        let expected = dtmod
            .call_method1("timedelta", (27_777, 18 * 3600 + 40 * 60, 0))
            .unwrap();
        let td = value.call_method0("to_timedelta").unwrap();
        assert!(td.eq(&expected).unwrap(), "{td} != {expected}");
    });
}

#[test]
fn test_extreme_durations_render_the_engine_iso_text() {
    with_py(|py| {
        for (dur, expected) in [
            (
                IsoDuration::new(0, 0, i64::MAX, 0),
                "PT2562047788015215H30M7S",
            ),
            (
                IsoDuration::new(0, 0, i64::MIN, 0),
                "PT-2562047788015215H-30M-8S",
            ),
            (
                IsoDuration::new(0, 106_751_991_167_300, 0, 0),
                "P106751991167300D",
            ),
        ] {
            let value = duration_to_py(py, &dur).unwrap().into_bound(py);
            assert_eq!(value.str().unwrap().to_string(), expected);
            assert!(value.call_method0("to_timedelta").is_err());
        }
    });
}

#[test]
fn test_timedelta_range_edges_project_and_overflow_raises() {
    with_py(|py| {
        let dtmod = py.import("datetime").unwrap();
        let timedelta = dtmod.getattr("timedelta").unwrap();
        for (dur, expected) in [
            (
                IsoDuration::new(0, 999_999_999, 86_399, 999_999_000),
                timedelta.getattr("max").unwrap(),
            ),
            (
                IsoDuration::new(0, -999_999_999, 0, 0),
                timedelta.getattr("min").unwrap(),
            ),
        ] {
            let value = duration_timedelta(py, dur).unwrap();
            assert!(value.eq(&expected).unwrap(), "{value:?}");
        }
        // One microsecond past `timedelta.max`: a typed `ValueError`.
        let err = duration_timedelta(py, IsoDuration::new(0, 999_999_999, 86_400, 0)).unwrap_err();
        assert!(err.is_instance_of::<PyValueError>(py), "{err}");
    });
}

#[test]
fn test_extreme_duration_query_does_not_raise_c_int_error() {
    with_py(|py| {
        let mut db = new_database(":memory:");
        // Regression: the results layer used to raise
        // `OverflowError: Python int too large to convert to C int` here.
        let value = query_first_py(
            py,
            &mut db,
            "RETURN duration('PT9223372036854775807S') AS d",
            "d",
        );
        assert_eq!(value.get_type().name().unwrap(), "Duration");
    });
}

#[test]
fn test_sub_microsecond_duration_result_is_lossless() {
    with_py(|py| {
        // `timedelta` holds whole microseconds; the `Duration` keeps every
        // nanosecond and renders the engine's ISO-8601 text.
        let (value, type_name) = temporal_result(py, "duration('PT5H6M7.00800901S')");
        assert_eq!(type_name, "Duration");
        assert_eq!(value.str().unwrap().to_string(), "PT5H6M7.00800901S");
        assert_eq!(temporal_attr::<i64>(&value, "nanoseconds") % 1_000, 10);
    });
}
