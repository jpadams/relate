//! The `float16-coordinate-types` preview gate, CLOSED side (ADR-0371,
//! CIP-280): without the opt-in every FLOAT16/BFLOAT16 use fails closed
//! with the authored disabled-feature error, in the `vector()` constructor
//! and in `VECTOR<…>` type expressions alike.
//!
//! This file is its own test binary: `RELATE_PREVIEW` is
//! process-global and read once per `Database::open` (ADR-0371), so it must
//! never share a process with the enabled side
//! (`float16_coordinate_types_gate.rs`).

mod common;

use common::{assert_preview_closed, connection};
use relate_core::PreviewFeature;

#[test]
fn vector_constructor_float16_needs_the_opt_in() {
    for query in [
        "RETURN VECTOR([1], 1, FLOAT16) AS v",
        "RETURN VECTOR([1], 1, BFLOAT16) AS v",
    ] {
        assert_preview_closed(PreviewFeature::Float16CoordinateTypes, query);
    }
}

#[test]
fn float16_type_expression_needs_the_opt_in() {
    for query in [
        "RETURN 1 IS :: VECTOR<FLOAT16>(3) AS r",
        "RETURN 1 IS :: VECTOR<BFLOAT16> AS r",
        "RETURN 1 IS :: LIST<VECTOR<FLOAT16>(3)> AS r",
        "CREATE CONSTRAINT v FOR (n:V) REQUIRE n.e IS :: VECTOR<FLOAT16>(3)",
    ] {
        assert_preview_closed(PreviewFeature::Float16CoordinateTypes, query);
    }
}

#[test]
fn other_coordinate_types_stay_open() {
    let mut c = connection();
    c.query("RETURN VECTOR([1], 1, FLOAT32) AS v")
        .expect("FLOAT32 is not gated");
}
