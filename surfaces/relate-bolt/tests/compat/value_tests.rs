use super::{matches, parse_fields, render};
use crate::common::packstream::Pack;

fn one(text: &str) -> Pack {
    let mut fields = parse_fields(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    assert_eq!(fields.len(), 1, "{text}");
    fields.remove(0)
}

fn s(text: &str) -> Pack {
    Pack::String(text.into())
}

fn map(entries: &[(&str, Pack)]) -> Pack {
    Pack::Map(entries.iter().map(|(k, v)| (s(k), v.clone())).collect())
}

#[test]
fn json_maps_to_packstream() {
    assert_eq!(one("1"), Pack::Int(1));
    assert_eq!(one("-7"), Pack::Int(-7));
    assert_eq!(one("1.0"), Pack::Float(1.0));
    assert_eq!(one("1e3"), Pack::Float(1000.0));
    assert_eq!(one("\"a\""), s("a"));
    assert_eq!(one("true"), Pack::Bool(true));
    assert_eq!(one("null"), Pack::Null);
    assert_eq!(one("[1, \"x\"]"), Pack::List(vec![Pack::Int(1), s("x")]));
    assert_eq!(
        one("{\"k\": [null]}"),
        map(&[("k", Pack::List(vec![Pack::Null]))])
    );
    assert_eq!(
        one("{\"#4E\": [1, [\"L\"], {}]}"),
        Pack::Struct {
            tag: 0x4E,
            fields: vec![Pack::Int(1), Pack::List(vec![s("L")]), Pack::Map(vec![])],
        }
    );
    // A `#NN` key among others is an ordinary map key.
    assert_eq!(
        one("{\"#4E\": 1, \"b\": 2}"),
        map(&[("#4E", Pack::Int(1)), ("b", Pack::Int(2))])
    );
}

#[test]
fn fields_are_whitespace_separated_json_values() {
    assert_eq!(
        parse_fields("\"RETURN 1\" {}  {}").unwrap(),
        vec![s("RETURN 1"), Pack::Map(vec![]), Pack::Map(vec![])]
    );
    assert_eq!(parse_fields("").unwrap(), vec![]);
}

#[test]
fn bad_json_values_are_errors() {
    assert!(parse_fields("{\"a\": }")
        .unwrap_err()
        .contains("malformed JSON"));
    assert!(parse_fields("RETURN")
        .unwrap_err()
        .contains("malformed JSON"));
    assert!(parse_fields("9223372036854775808")
        .unwrap_err()
        .contains("does not fit a PackStream Int"));
}

#[test]
fn integer_literals_outside_i64_are_errors_not_floats() {
    // serde_json reads a literal above u64::MAX as an f64; it must not
    // become a Float.
    for text in [
        "18446744073709551616",
        "-9223372036854775809",
        "\"RETURN 1\" {\"n\": [1, {\"#4E\": [18446744073709551616]}]}",
    ] {
        let error = parse_fields(text).expect_err(text);
        assert!(
            error.contains("does not fit a PackStream Int (i64)"),
            "{text}: {error}"
        );
    }
    assert_eq!(one("-9223372036854775808"), Pack::Int(i64::MIN));
    assert_eq!(
        one("1.8446744073709552e19"),
        Pack::Float(18446744073709551616.0)
    );
    assert_eq!(
        one("18446744073709551616.0"),
        Pack::Float(18446744073709551616.0)
    );
    // Digits in a string, escapes included, are not number tokens.
    assert_eq!(
        one(r#""\"18446744073709551616""#),
        s("\"18446744073709551616")
    );
}

#[test]
fn expected_maps_match_a_subset_of_keys() {
    let actual = map(&[
        ("fields", Pack::List(vec![s("x")])),
        ("t_first", Pack::Int(3)),
    ]);
    assert!(matches(&one("{}"), &actual));
    assert!(matches(&one("{\"fields\": [\"x\"]}"), &actual));
    assert!(!matches(&one("{\"fields\": [\"y\"]}"), &actual));
    assert!(
        !matches(&one("{\"qid\": 0}"), &actual),
        "a listed key must be present"
    );
}

#[test]
fn wildcard_map_value_matches_anything_present() {
    let actual = map(&[("t_first", Pack::Int(3)), ("db", s("neo4j"))]);
    assert!(matches(
        &one("{\"t_first\": \"*\", \"db\": \"*\"}"),
        &actual
    ));
    assert!(!matches(&one("{\"bookmark\": \"*\"}"), &actual));
    // Only as a map value: in a list `"*"` is the literal string.
    assert!(!matches(&one("[\"*\"]"), &Pack::List(vec![Pack::Int(1)])));
}

#[test]
fn lists_scalars_and_structures_match_exactly() {
    let record = |values| Pack::Struct {
        tag: 0x71,
        fields: vec![Pack::List(values)],
    };
    let want = one("{\"#71\": [[1, {\"a\": 1}]]}");
    assert!(matches(
        &want,
        &record(vec![
            Pack::Int(1),
            map(&[("a", Pack::Int(1)), ("b", Pack::Int(2))])
        ])
    ));
    assert!(
        !matches(&want, &record(vec![Pack::Int(1)])),
        "lengths differ"
    );
    assert!(!matches(
        &want,
        &record(vec![Pack::Int(1), map(&[("a", Pack::Int(1))]), Pack::Null])
    ));
    assert!(
        !matches(&one("{\"#70\": [[1, {\"a\": 1}]]}"), &record(vec![])),
        "tags differ"
    );
    assert!(!matches(&one("1"), &Pack::Float(1.0)), "Int is not Float");
    assert!(!matches(&one("{}"), &Pack::List(vec![])));
}

#[test]
fn render_writes_script_notation() {
    let message = Pack::Struct {
        tag: 0x4E,
        fields: vec![
            Pack::Int(1),
            Pack::Float(2.0),
            s("q\""),
            Pack::Null,
            map(&[("k", Pack::Bool(false))]),
            Pack::Bytes(vec![0xAB]),
        ],
    };
    assert_eq!(
        render(&message),
        r##"{"#4E": [1, 2.0, "q\"", null, {"k": false}, <bytes AB>]}"##
    );
}
