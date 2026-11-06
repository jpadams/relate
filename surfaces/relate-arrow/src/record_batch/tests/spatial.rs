use super::super::*;
use super::one_col;
use crate::from_arrow::record_batch_to_rows;
use relate_model::value::{GeometryValue, PointValue};

fn line(srid: i32) -> GeometryValue {
    GeometryValue::LineString {
        coordinates: vec![[1.25, -2.5], [3.0, 4.75]],
        srid,
    }
}

fn collection(geometries: Vec<GeometryValue>, srid: i32) -> GeometryValue {
    GeometryValue::GeometryCollection { geometries, srid }
}

fn wrap_in_collections(mut geometry: GeometryValue, count: usize) -> GeometryValue {
    for _ in 0..count {
        geometry = collection(vec![geometry], 4326);
    }
    geometry
}

#[test]
fn nonfinite_spatial_coordinates_cannot_be_written_as_typed_json() {
    let bad_coord = [1.0, f64::INFINITY];
    let invalid = [
        CypherValue::Point(PointValue {
            srid: 7203,
            x: f64::NAN,
            y: 2.0,
            z: None,
        }),
        CypherValue::Point(PointValue {
            srid: 7203,
            x: 1.0,
            y: f64::INFINITY,
            z: None,
        }),
        CypherValue::Point(PointValue {
            srid: 7203,
            x: 1.0,
            y: 2.0,
            z: Some(f64::NEG_INFINITY),
        }),
        CypherValue::Geometry(GeometryValue::LineString {
            coordinates: vec![bad_coord],
            srid: 7203,
        }),
        CypherValue::Geometry(GeometryValue::Polygon {
            rings: vec![vec![bad_coord]],
            srid: 7203,
        }),
        CypherValue::Geometry(GeometryValue::MultiPoint {
            points: vec![bad_coord],
            srid: 7203,
        }),
        CypherValue::Geometry(GeometryValue::MultiLineString {
            lines: vec![vec![bad_coord]],
            srid: 7203,
        }),
        CypherValue::Geometry(GeometryValue::MultiPolygon {
            polygons: vec![vec![vec![bad_coord]]],
            srid: 7203,
        }),
        CypherValue::Geometry(GeometryValue::GeometryCollection {
            geometries: vec![GeometryValue::LineString {
                coordinates: vec![bad_coord],
                srid: 7203,
            }],
            srid: 7203,
        }),
    ];
    for bad in invalid {
        let (cols, rows) = one_col(
            "spatial",
            vec![CypherValue::Map(IndexMap::from([(
                "nested".into(),
                CypherValue::List(vec![bad.clone()]),
            )]))],
        );
        for error in [
            result_to_record_batch(&cols, &rows).unwrap_err(),
            result_to_record_batches(&cols, &rows, 1).unwrap_err(),
        ] {
            assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
            assert!(
                error.to_string().contains("column 'spatial' row 0"),
                "{error}"
            );
            assert!(
                error.to_string().contains("expected finite number"),
                "{error}"
            );
            assert!(error.to_string().contains("$."), "{error}");
        }
    }
}

#[test]
fn point_maps_to_wkb_binary_with_geoarrow_metadata() {
    use crate::geo_encoding::wkb_to_cypher_value;
    let (cols, rows) = one_col(
        "p",
        vec![CypherValue::Point(PointValue::cartesian_2d(1.5, 2.5))],
    );
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(batch.schema().field(0).data_type(), &DataType::Binary);
    // GeoArrow extension metadata proves the WKB encoder path was used.
    let schema = batch.schema();
    let md = schema.field(0).metadata();
    assert_eq!(
        md.get("ARROW:extension:name").map(|s| s.as_str()),
        Some("geoarrow.wkb")
    );
    assert_eq!(md.get(RELATE_SRID_KEY).map(|s| s.as_str()), Some("7203"));
    // And the bytes decode back to the original point.
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::BinaryArray>()
        .unwrap();
    let decoded = wkb_to_cypher_value(arr.value(0), 7203).unwrap();
    match decoded {
        CypherValue::Point(p) => {
            assert!((p.x - 1.5).abs() < 1e-9);
            assert!((p.y - 2.5).abs() < 1e-9);
        }
        other => panic!("expected Point, got {other:?}"),
    }
}

#[test]
fn three_dimensional_points_use_typed_json_with_per_cell_srids() {
    let three_d = CypherValue::Point(PointValue::wgs84_3d(1.25, -2.5, 75.0));
    let two_d = CypherValue::Point(PointValue::cartesian_2d(10.0, 20.0));
    let geometry = CypherValue::Geometry(collection(vec![line(4326)], 4326));
    // Standard WKB cannot carry this child's different SRID. A 3D Point
    // selects typed JSON for the column, which preserves both SRIDs.
    let nested_mixed_srid = CypherValue::Geometry(collection(vec![line(7203)], 4326));
    for values in [
        vec![three_d.clone()],
        vec![two_d.clone(), CypherValue::Null, three_d.clone()],
        vec![geometry.clone(), two_d.clone(), three_d.clone()],
        vec![three_d.clone(), geometry.clone()],
        vec![three_d.clone(), nested_mixed_srid],
    ] {
        let (cols, rows) = one_col("location", values);
        assert_eq!(
            schema::infer_column_type("location", &rows),
            ColType::TypedJson
        );
        for batches in [
            vec![result_to_record_batch(&cols, &rows).unwrap()],
            result_to_record_batches(&cols, &rows, 1).unwrap(),
        ] {
            for batch in &batches {
                let field = batch.schema().field(0).clone();
                assert_eq!(field.data_type(), &DataType::Utf8);
                assert_eq!(
                    field.metadata()[RELATE_ENCODING_KEY],
                    ENCODING_TYPED_JSON_V1
                );
                assert!(!field.metadata().contains_key(RELATE_SRID_KEY));
                assert_eq!(batch.schema(), batches[0].schema());
            }
            let encoded = batches
                .iter()
                .flat_map(|batch| {
                    let array = batch
                        .column(0)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .unwrap();
                    (0..array.len())
                        .filter(|&i| !array.is_null(i))
                        .map(|i| serde_json::from_str::<serde_json::Value>(array.value(i)).unwrap())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let point_payload = encoded
                .iter()
                .find(|json| json["value"]["z"] == 75.0)
                .unwrap();
            assert_eq!(point_payload["type"], "POINT");
            assert_eq!(point_payload["value"]["srid"], 4979);
            assert_eq!(point_payload["value"]["x"], 1.25);
            assert_eq!(point_payload["value"]["y"], -2.5);
            assert_eq!(point_payload["value"]["z"], 75.0);
            let decoded = batches
                .iter()
                .flat_map(|batch| record_batch_to_rows(batch).unwrap().1)
                .collect::<Vec<_>>();
            assert_eq!(decoded, rows);
        }
    }
}

#[test]
fn three_dimensional_typed_json_rejects_bad_z_in_later_chunk() {
    let (cols, rows) = one_col(
        "location",
        vec![
            CypherValue::Point(PointValue::cartesian_2d(1.0, 2.0)),
            CypherValue::Point(PointValue {
                srid: 4979,
                x: 3.0,
                y: 4.0,
                z: Some(f64::INFINITY),
            }),
        ],
    );
    assert_eq!(
        schema::infer_column_type("location", &rows),
        ColType::TypedJson
    );
    for error in [
        result_to_record_batch(&cols, &rows).unwrap_err(),
        result_to_record_batches(&cols, &rows, 1).unwrap_err(),
    ] {
        assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
        assert!(
            error.to_string().contains("column 'location' row 1"),
            "{error}"
        );
        assert!(error.to_string().contains("$.z"), "{error}");
        assert!(
            error.to_string().contains("expected finite number"),
            "{error}"
        );
    }
}

#[test]
fn two_dimensional_point_and_geometry_still_use_native_wkb() {
    let (cols, rows) = one_col(
        "location",
        vec![
            CypherValue::Point(PointValue::cartesian_2d(1.0, 2.0)),
            CypherValue::Geometry(line(7203)),
            CypherValue::Null,
        ],
    );
    assert_eq!(
        schema::infer_column_type("location", &rows),
        ColType::Spatial
    );
    for batches in [
        vec![result_to_record_batch(&cols, &rows).unwrap()],
        result_to_record_batches(&cols, &rows, 1).unwrap(),
    ] {
        for batch in &batches {
            assert_eq!(batch.schema().field(0).data_type(), &DataType::Binary);
            assert_eq!(batch.schema().field(0).metadata()[RELATE_SRID_KEY], "7203");
            assert_eq!(
                batch.schema().field(0).metadata()["ARROW:extension:name"],
                GEOARROW_EXTENSION_NAME
            );
        }
        assert_eq!(
            batches
                .iter()
                .flat_map(|batch| record_batch_to_rows(batch).unwrap().1)
                .collect::<Vec<_>>(),
            rows
        );
    }
}

#[test]
fn native_wkb_preflight_rejects_nonfinite_coordinates_at_every_depth() {
    let invalid = [
        (
            CypherValue::Point(PointValue {
                srid: 7203,
                x: f64::NAN,
                y: 2.0,
                z: None,
            }),
            "coordinate $ x",
        ),
        (
            CypherValue::Point(PointValue {
                srid: 7203,
                x: 1.0,
                y: f64::INFINITY,
                z: None,
            }),
            "coordinate $ y",
        ),
        (
            CypherValue::Geometry(GeometryValue::LineString {
                coordinates: vec![[0.0, 1.0], [f64::NEG_INFINITY, 2.0]],
                srid: 7203,
            }),
            "coordinate [1] x",
        ),
        (
            CypherValue::Geometry(GeometryValue::Polygon {
                rings: vec![vec![[0.0, 1.0]], vec![[1.0, f64::NAN]]],
                srid: 7203,
            }),
            "coordinate [1][0] y",
        ),
        (
            CypherValue::Geometry(GeometryValue::MultiPoint {
                points: vec![[0.0, 1.0], [f64::NAN, 1.0]],
                srid: 7203,
            }),
            "coordinate [1] x",
        ),
        (
            CypherValue::Geometry(GeometryValue::MultiLineString {
                lines: vec![vec![[0.0, 1.0]], vec![[1.0, f64::INFINITY]]],
                srid: 7203,
            }),
            "coordinate [1][0] y",
        ),
        (
            CypherValue::Geometry(GeometryValue::MultiPolygon {
                polygons: vec![vec![vec![[0.0, 1.0]]], vec![vec![[f64::NEG_INFINITY, 1.0]]]],
                srid: 7203,
            }),
            "coordinate [1][0][0] x",
        ),
        (
            CypherValue::Geometry(collection(
                vec![
                    line(7203),
                    collection(
                        vec![GeometryValue::MultiLineString {
                            lines: vec![vec![[1.0, f64::NAN]]],
                            srid: 7203,
                        }],
                        7203,
                    ),
                ],
                7203,
            )),
            "coordinate [1][0][0][0] y",
        ),
    ];
    for (bad, location) in invalid {
        // Row 2 belongs to a later chunk; both entry points preflight before
        // constructing any batch, with absolute row and nested coordinate path.
        let (cols, rows) = one_col(
            "location",
            vec![
                CypherValue::Point(PointValue::cartesian_2d(1.0, 2.0)),
                CypherValue::Null,
                bad,
            ],
        );
        for error in [
            result_to_record_batch(&cols, &rows).unwrap_err(),
            result_to_record_batches(&cols, &rows, 1).unwrap_err(),
        ] {
            assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
            assert!(
                error.to_string().contains("column 'location' row 2"),
                "{error}"
            );
            assert!(error.to_string().contains(location), "{error}");
            assert!(error.to_string().contains("must be finite"), "{error}");
        }
    }
}

#[test]
fn spatial_plan_requires_srid_and_always_emits_metadata() {
    let (cols, rows) = one_col(
        "p",
        vec![CypherValue::Point(PointValue::cartesian_2d(1.0, 2.0))],
    );
    let mut plan = ColPlan {
        name: cols[0].clone(),
        kind: ColType::Spatial,
        srid: None,
        tz: None,
    };
    let err = build_column(&plan, &rows, 0).unwrap_err();
    assert!(matches!(err, ArrowError::InvalidArgumentError(_)));
    assert!(err.to_string().contains("column 'p'"), "{err}");
    assert!(err.to_string().contains("missing its SRID"), "{err}");
    assert!(spatial_srid("p", &[IndexMap::new()]).is_err());

    plan.srid = Some(7203);
    let (_, field) = build_column(&plan, &rows, 0).unwrap();
    assert_eq!(
        field.metadata().get(RELATE_SRID_KEY).map(String::as_str),
        Some("7203")
    );
    assert_eq!(
        field
            .metadata()
            .get("ARROW:extension:metadata")
            .map(String::as_str),
        Some("{\"crs\":\"EPSG:7203\"}")
    );
}

#[test]
fn geometry_maps_to_wkb_binary() {
    let (cols, rows) = one_col(
        "g",
        vec![CypherValue::Geometry(GeometryValue::LineString {
            coordinates: vec![[0.0, 0.0], [1.0, 1.0]],
            srid: 4326,
        })],
    );
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(batch.schema().field(0).data_type(), &DataType::Binary);
    assert_eq!(
        batch
            .schema()
            .field(0)
            .metadata()
            .get(RELATE_SRID_KEY)
            .map(|s| s.as_str()),
        Some("4326")
    );
}

#[test]
fn spatial_srid_checks_all_rows_even_across_chunks() {
    let geometry = |srid| {
        CypherValue::Geometry(GeometryValue::LineString {
            coordinates: vec![[1.0, 2.0]],
            srid,
        })
    };
    let point = CypherValue::Point(PointValue::cartesian_2d(1.0, 2.0));
    let (cols, rows) = one_col(
        "location",
        vec![CypherValue::Null, point.clone(), geometry(4326)],
    );
    for error in [
        result_to_record_batch(&cols, &rows).unwrap_err(),
        result_to_record_batches(&cols, &rows, 1).unwrap_err(),
    ] {
        assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
        for part in ["location", "7203", "4326"] {
            assert!(error.to_string().contains(part), "{error}");
        }
    }
    let (cols, rows) = one_col("location", vec![CypherValue::Null, point, geometry(7203)]);
    let batches = result_to_record_batches(&cols, &rows, 1).unwrap();
    assert_eq!(batches.len(), 3);
    for batch in batches {
        assert_eq!(batch.schema().field(0).data_type(), &DataType::Binary);
        assert_eq!(
            batch
                .schema()
                .field(0)
                .metadata()
                .get(RELATE_SRID_KEY)
                .map(String::as_str),
            Some("7203")
        );
        assert_eq!(
            batch
                .schema()
                .field(0)
                .metadata()
                .get("ARROW:extension:name")
                .map(String::as_str),
            Some(GEOARROW_EXTENSION_NAME)
        );
    }
}

#[test]
fn spatial_srid_rejects_mismatched_collection_child() {
    let (cols, rows) = one_col(
        "location",
        vec![CypherValue::Geometry(collection(
            vec![line(4326), line(7203)],
            4326,
        ))],
    );
    for error in [
        result_to_record_batch(&cols, &rows).unwrap_err(),
        result_to_record_batches(&cols, &rows, 1).unwrap_err(),
    ] {
        assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
        for part in [
            "column 'location' row 0",
            "member [1]",
            "SRID 7203",
            "parent SRID 4326",
            "field SRID 4326",
        ] {
            assert!(error.to_string().contains(part), "{error}");
        }
    }
}

#[test]
fn spatial_srid_rejects_mismatched_collection_grandchild() {
    let (cols, rows) = one_col(
        "location",
        vec![CypherValue::Geometry(collection(
            vec![line(4326), collection(vec![line(7203)], 4326)],
            4326,
        ))],
    );
    for error in [
        result_to_record_batch(&cols, &rows).unwrap_err(),
        result_to_record_batches(&cols, &rows, 1).unwrap_err(),
    ] {
        assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
        for part in [
            "column 'location' row 0",
            "member [1][0]",
            "SRID 7203",
            "parent SRID 4326",
            "field SRID 4326",
        ] {
            assert!(error.to_string().contains(part), "{error}");
        }
    }
}

#[test]
fn spatial_srid_rejects_mismatched_collection_in_later_chunk() {
    let (cols, rows) = one_col(
        "location",
        vec![
            CypherValue::Point(PointValue {
                srid: 4326,
                x: 5.0,
                y: 6.0,
                z: None,
            }),
            CypherValue::Null,
            CypherValue::Geometry(collection(vec![line(4326)], 4326)),
            CypherValue::Geometry(collection(
                vec![line(4326), collection(vec![line(7203)], 4326)],
                4326,
            )),
        ],
    );
    for error in [
        result_to_record_batch(&cols, &rows).unwrap_err(),
        result_to_record_batches(&cols, &rows, 2).unwrap_err(),
    ] {
        assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
        for part in ["column 'location' row 3", "[1][0]", "4326", "7203"] {
            assert!(error.to_string().contains(part), "{error}");
        }
    }
}

#[test]
fn uniform_nested_collection_roundtrips_every_srid_and_coordinate() {
    let nested = CypherValue::Geometry(collection(
        vec![
            line(4326),
            collection(
                vec![
                    GeometryValue::MultiPoint {
                        points: vec![[0.125, -90.5], [180.0, 45.0]],
                        srid: 4326,
                    },
                    collection(vec![line(4326)], 4326),
                ],
                4326,
            ),
        ],
        4326,
    ));
    let (cols, rows) = one_col("location", vec![nested, CypherValue::Null]);
    let single = result_to_record_batch(&cols, &rows).unwrap();
    let (decoded_cols, decoded_rows) = record_batch_to_rows(&single).unwrap();
    assert_eq!(decoded_cols, cols);
    assert_eq!(decoded_rows, rows);
    assert_eq!(
        single.schema().field(0).metadata().get(RELATE_SRID_KEY),
        Some(&"4326".to_owned())
    );

    let batches = result_to_record_batches(&cols, &rows, 1).unwrap();
    assert_eq!(batches.len(), 2);
    for batch in &batches {
        assert_eq!(batch.schema(), single.schema());
    }
    let chunked_rows = batches
        .iter()
        .flat_map(|batch| record_batch_to_rows(batch).unwrap().1)
        .collect::<Vec<_>>();
    assert_eq!(chunked_rows, rows);
}

#[test]
fn spatial_wkb_depth_64_collection_roundtrips() {
    // The empty leaf collection has a WKB header at level 64.
    let nested = wrap_in_collections(collection(vec![], 4326), 63);
    let (cols, rows) = one_col("location", vec![CypherValue::Geometry(nested)]);
    let batch = result_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(record_batch_to_rows(&batch).unwrap().1, rows);
    let chunks = result_to_record_batches(&cols, &rows, 1).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(record_batch_to_rows(&chunks[0]).unwrap().1, rows);
}

#[test]
fn spatial_wkb_depth_65_rejected_in_single_and_later_chunk() {
    let nested = wrap_in_collections(collection(vec![], 4326), 64);
    let (cols, rows) = one_col(
        "location",
        vec![
            CypherValue::Point(PointValue {
                srid: 4326,
                x: 1.0,
                y: 2.0,
                z: None,
            }),
            CypherValue::Null,
            CypherValue::Geometry(nested),
        ],
    );
    for error in [
        result_to_record_batch(&cols, &rows).unwrap_err(),
        result_to_record_batches(&cols, &rows, 1).unwrap_err(),
    ] {
        assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
        for part in [
            "column 'location' row 2",
            &"[0]".repeat(64),
            "depth 65",
            "64",
        ] {
            assert!(error.to_string().contains(part), "{error}");
        }
    }
}

#[test]
fn spatial_wkb_multi_member_headers_obey_depth_boundary() {
    let multis = [
        GeometryValue::MultiPoint {
            points: vec![[1.25, -2.5]],
            srid: 4326,
        },
        GeometryValue::MultiLineString {
            lines: vec![vec![[1.25, -2.5]]],
            srid: 4326,
        },
        GeometryValue::MultiPolygon {
            polygons: vec![vec![vec![[1.25, -2.5]]]],
            srid: 4326,
        },
    ];
    for multi in multis {
        // At level 63, the implicit member header is level 64 and decodes.
        let (cols, rows) = one_col(
            "location",
            vec![CypherValue::Geometry(wrap_in_collections(
                multi.clone(),
                62,
            ))],
        );
        let batch = result_to_record_batch(&cols, &rows).unwrap();
        assert_eq!(record_batch_to_rows(&batch).unwrap().1, rows);

        // At level 64, the first implicit member header would be level 65.
        let (cols, rows) = one_col(
            "location",
            vec![CypherValue::Geometry(wrap_in_collections(multi, 63))],
        );
        for error in [
            result_to_record_batch(&cols, &rows).unwrap_err(),
            result_to_record_batches(&cols, &rows, 1).unwrap_err(),
        ] {
            assert!(matches!(error, ArrowError::InvalidArgumentError(_)));
            for part in ["column 'location' row 0", &"[0]".repeat(64), "depth 65"] {
                assert!(error.to_string().contains(part), "{error}");
            }
        }
    }
}
