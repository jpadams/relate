//! The `float16-coordinate-types` preview gate, ENABLED side (ADR-0371,
//! CIP-280): with `RELATE_PREVIEW=float16-coordinate-types` the
//! FLOAT16/BFLOAT16 vector coordinate types work in the `vector()`
//! constructor and in `VECTOR<…>` type expressions, and stay distinct
//! coordinate kinds.
//!
//! `RELATE_PREVIEW` is process-global and read once per
//! `Database::open` (ADR-0371), so the enabled and closed sides of the
//! gate are separate test binaries: `float16_coordinate_types_closed.rs`
//! pins the authored disabled-feature error and must never share a process
//! with this file.

mod common;

use relate::Value;

use common::{connection, set_preview_once};

#[test]
fn float16_and_bfloat16_vector_coordinate_kinds_are_distinct() {
    set_preview_once("float16-coordinate-types");
    let mut c = connection();
    let r = c
        .query(
            "RETURN VECTOR([1, 2, 3], 3, FLOAT16) IS :: VECTOR<FLOAT16>(3) AS a, \
             VECTOR([1, 2, 3], 3, FLOAT16) IS :: VECTOR<BFLOAT16> AS b, \
             VECTOR([257], 1, BFLOAT16) IS :: VECTOR<BFLOAT16>(1) AS d",
        )
        .unwrap();
    let [row] = &r.rows[..] else {
        panic!("expected one row, got {:?}", r.rows)
    };
    assert_eq!(
        row.as_slice(),
        [Value::Bool(true), Value::Bool(false), Value::Bool(true)]
    );
}

#[test]
fn bfloat16_orders_before_float16() {
    // CIP-200's coordinate-type order is BFLOAT16 < FLOAT16 (Neo4j's
    // `CypherTypeOrder`); VectorValueAcceptance's "Ordering of Vectors
    // amongst other types" pins the same relative order.
    set_preview_once("float16-coordinate-types");
    let mut c = connection();
    let r = c
        .query(
            "UNWIND [VECTOR([1.0, 2.0, 1.0, 4.0, 5.0], 5, FLOAT16), \
             VECTOR([1.0, 2.0, 1.0, 4.0, 5.0], 5, BFLOAT16)] AS x \
             RETURN x ORDER BY x",
        )
        .unwrap();
    let kinds: Vec<_> = r
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::Vector { coord, .. } => *coord,
            other => panic!("expected a vector, got {other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            relate_core::VectorCoordKind::BFloat16,
            relate_core::VectorCoordKind::Float16
        ]
    );
}

#[test]
fn float16_coordinate_types_work_in_declared_property_types() {
    set_preview_once("float16-coordinate-types");
    let mut c = connection();
    c.query("CREATE CONSTRAINT v FOR (n:V) REQUIRE n.embedding IS :: VECTOR<FLOAT16>(3)")
        .expect("a FLOAT16-typed property constraint lowers behind the gate");
}

#[test]
fn float32_coordinates_need_no_opt_in() {
    // The gate covers only FLOAT16/BFLOAT16; the other coordinate kinds
    // stay on the dialect floor.
    set_preview_once("float16-coordinate-types");
    let mut c = connection();
    let r = c.query("RETURN VECTOR([1], 1, FLOAT32) AS v").unwrap();
    assert!(matches!(r.rows[0][0], Value::Vector { .. }));
}
