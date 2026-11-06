//! `point()` construction rejections carry the error *class* Neo4j
//! surfaces. The live-verified class table — which shapes are
//! `ArgumentError`, which `SyntaxError`, with the GQLSTATUS codes and the
//! probe that settled them — lives in the executor's `spatial` module
//! doc (`relate-executor/src/spatial/mod.rs`), the one place a code
//! reader meets it; this table only pins the classes per site.

mod common;

use common::{connection, error_class, error_class_literal, map, params};
use relate::Value;

/// The in-map value-domain sites, one row per site: each row carries a
/// statement that can only fail at that site, so the rows are not
/// interchangeable — a rejection from the wrong site fails its row.
#[test]
fn point_value_domain_rejections_are_argument_errors() {
    let mut conn = connection();
    for statement in [
        // unknown srid code (`Srid::from_i64`)
        "RETURN point({srid: 12345, x: 2.3, y: 4.5}) AS p",
        // unknown crs name (`Srid::from_name`)
        "RETURN point({x: 2.3, y: 4.5, crs: 'cart'}) AS p",
        // out-of-range latitude
        "RETURN point({longitude: 0.0, latitude: 91.0}) AS p",
        // `crs`/`srid` conflict
        "RETURN point({crs: 'cartesian', srid: 4326, x: 2.3, y: 4.5}) AS p",
        // srid does not match the coordinate dimension
        "RETURN point({srid: 9157, x: 2.3, y: 4.5}) AS p",
        // geographic keys with a cartesian srid
        "RETURN point({srid: 7203, longitude: 2.3, latitude: 4.5}) AS p",
    ] {
        assert_eq!(
            error_class_literal(&mut conn, statement),
            "ArgumentError",
            "{statement}"
        );
    }
}

/// Missing coordinate keys reach the executor only through a runtime map
/// (a parameter or an entity's properties): a *literal* map is rejected
/// earlier by the planner's `point` shape rule (SyntaxError), which the
/// corpus does not dispute. The parameter spelling here is the corpus's
/// own shape (`point($params)`).
#[test]
fn point_missing_keys_through_a_parameter_is_an_argument_error() {
    let mut conn = connection();
    // One row per missing key: (the map's entries, what is missing).
    for (entries, missing) in [
        (vec![("y", Value::Float(1.0))], "x"),
        (vec![("latitude", Value::Float(1.0))], "longitude"),
        (vec![("longitude", Value::Float(1.0))], "latitude"),
    ] {
        let statement = "RETURN point($params) AS p";
        let value = map(&entries);
        assert_eq!(
            error_class(&mut conn, statement, params(&[("params", value)])),
            "ArgumentError",
            "missing {missing}"
        );
    }
}

/// A non-finite coordinate reaches the same class through a parameter —
/// the corpus's own shape (`y: $v` with `v` = NaN/±Infinity).
#[test]
fn point_non_finite_coordinate_is_an_argument_error() {
    let mut conn = connection();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let statement = "RETURN point({x: 2.3, y: $v}) AS p";
        assert_eq!(
            error_class(&mut conn, statement, params(&[("v", Value::Float(value))])),
            "ArgumentError",
            "y = {value}"
        );
    }
}

/// The in-map type rows carry the same `ArgumentError` class (the
/// corpus's `x: 'apa'` and `crs: 5` rows pin it; the class table in the
/// `spatial` module doc records the GQLSTATUS family).
#[test]
fn point_wrong_typed_map_arguments_are_argument_errors() {
    let mut conn = connection();
    for statement in [
        "RETURN point({x: '2.3', y: 4.5}) AS p",
        "RETURN point({srid: '4326', x: 2.3, y: 4.5}) AS p",
        "RETURN point({x: 2.3, y: 4.5, crs: 5}) AS p",
    ] {
        assert_eq!(
            error_class_literal(&mut conn, statement),
            "ArgumentError",
            "{statement}"
        );
    }
}

/// The non-map argument is the one `point()` rejection outside the
/// ArgumentError family: the planner's shape rule rejects the literal
/// spellings at compile time and the executor's backstop the runtime
/// ones, both in the SyntaxError class (live Neo4j agrees for the
/// parameter spelling too).
#[test]
fn point_non_map_argument_is_a_syntax_error() {
    let mut conn = connection();
    for statement in ["RETURN point(5) AS p", "RETURN point('foo') AS p"] {
        assert_eq!(
            error_class_literal(&mut conn, statement),
            "SyntaxError",
            "{statement}"
        );
    }
}

/// A *literal* map with an incomplete key set is the planner's static
/// `InvalidPoint` rule — SyntaxError, ahead of the executor's argument
/// family (the corpus's missing-key rows ride parameters and entities,
/// never literals).
#[test]
fn point_incomplete_literal_map_is_the_planners_syntax_error() {
    let mut conn = connection();
    assert_eq!(
        error_class_literal(&mut conn, "RETURN point({x: 2.3}) AS p"),
        "SyntaxError"
    );
}
