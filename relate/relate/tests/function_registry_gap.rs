//! Pins for the function/type vocabulary the parser-syntax-gap round added,
//! end-to-end through the public API: `isEmpty()`, `haversin()`,
//! `toStringList()`, the comma spelling of the GQL trim specification, and
//! the FLOAT32 literal suffix (`1.0f`, NaNAcceptance). The narrow float
//! vector coordinate kinds (`FLOAT16`/`BFLOAT16`) sit behind the
//! `float16-coordinate-types` preview gate and are pinned in
//! `float16_coordinate_types_gate.rs` (open) and
//! `float16_coordinate_types_closed.rs` (closed) -- this binary never sets
//! `RELATE_PREVIEW`, so it must stay in its own file.

mod common;

use relate::Value;

use common::connection;

#[test]
fn is_empty_reads_lists_maps_and_strings() {
    let mut c = connection();
    let r = c
        .query(
            "RETURN isEmpty(null) AS a, isEmpty([]) AS b, isEmpty([1]) AS c, \
             isEmpty({}) AS d, isEmpty({x: 1}) AS e, isEmpty('') AS f, \
             isEmpty('hello') AS g",
        )
        .unwrap();
    let [row] = &r.rows[..] else {
        panic!("expected one row, got {:?}", r.rows)
    };
    let expected = [
        Value::Null,
        Value::Bool(true),
        Value::Bool(false),
        Value::Bool(true),
        Value::Bool(false),
        Value::Bool(true),
        Value::Bool(false),
    ];
    assert_eq!(row.as_slice(), expected.as_slice());
}

#[test]
fn haversin_is_the_haversine_of_its_argument() {
    let mut c = connection();
    let r = c
        .query("RETURN haversin(0) AS a, round(haversin(1), 5) AS b, haversin(null) AS c")
        .unwrap();
    let [row] = &r.rows[..] else {
        panic!("expected one row, got {:?}", r.rows)
    };
    assert_eq!(row[0], Value::Float(0.0));
    assert_eq!(row[1], Value::Float(0.22985));
    assert_eq!(row[2], Value::Null);
}

#[test]
fn to_string_list_converts_its_elements() {
    let mut c = connection();
    let r = c
        .query("RETURN toStringList([1, [2, 3], {a: 1}]) AS xs, toStringList(null) AS n")
        .unwrap();
    let [row] = &r.rows[..] else {
        panic!("expected one row, got {:?}", r.rows)
    };
    assert_eq!(
        row[0],
        Value::List(
            vec![
                Value::String("1".into()),
                Value::String("[2, 3]".into()),
                Value::String("{a: 1}".into()),
            ]
            .into()
        )
    );
    assert_eq!(row[1], Value::Null);
}

#[test]
fn trim_accepts_the_comma_specification_spelling() {
    let mut c = connection();
    let r = c
        .query(
            "WITH 'xxxhelloxxx' AS s \
             RETURN trim('leading', 'x', s) AS a, trim('TRAILING', 'x', s) AS b, \
             trim('Both', 'x', s) AS d, trim('both', ' xy') AS e",
        )
        .unwrap();
    let [row] = &r.rows[..] else {
        panic!("expected one row, got {:?}", r.rows)
    };
    assert_eq!(row[0], Value::String("helloxxx".into()));
    assert_eq!(row[1], Value::String("xxxhello".into()));
    assert_eq!(row[2], Value::String("hello".into()));
    // Without a trim character string the whitespace default applies.
    assert_eq!(row[3], Value::String("xy".into()));
}

#[test]
fn float32_literal_suffix_lexes_as_a_float() {
    let mut c = connection();
    let r = c.query("RETURN isNaN(1.0f) AS a").unwrap();
    let [row] = &r.rows[..] else {
        panic!("expected one row, got {:?}", r.rows)
    };
    assert_eq!(row[0], Value::Bool(false));
}

#[test]
fn square_root_extreme_separated_literals_execute() {
    let mut c = connection();
    let r = c
        .query(
            "RETURN round(sqrt(9_223_372_036_854_775_807), 5) AS a, \
             sqrt(-9_223_372_036_854_775_808) AS b",
        )
        .unwrap();
    let [row] = &r.rows[..] else {
        panic!("expected one row, got {:?}", r.rows)
    };
    assert_eq!(row[0], Value::Float(3037000499.97605));
    assert!(matches!(row[1], Value::Float(f) if f.is_nan()));
}

#[test]
fn new_functions_are_advertised_by_show_functions() {
    let mut c = connection();
    let r = c
        .query(
            "SHOW FUNCTIONS YIELD name \
             WHERE name IN ['isEmpty', 'haversin', 'toStringList'] RETURN name",
        )
        .unwrap();
    let mut names: Vec<String> = r
        .rows
        .iter()
        .flatten()
        .map(|name| {
            name.as_string()
                .expect("SHOW FUNCTIONS name is a string")
                .to_ascii_lowercase()
        })
        .collect();
    names.sort();
    assert_eq!(names, ["haversin", "isempty", "tostringlist"]);
}
