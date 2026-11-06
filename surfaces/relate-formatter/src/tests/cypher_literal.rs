//! 8.8: shared `cypher_literal` / `cypher_identifier` — literal rendering
//! round-trips through the lexer/parser, identifiers are backticked when
//! needed, and map keys ride the same rules.

use super::*;
use relate_parser::{Clause, Expr, LiteralValue};

#[test]
fn test_float_literal_preserves_type() {
    // `1.0` must NOT collapse to `1` (the old `format!("{}", f)` bug): a
    // whole-valued float has to keep its decimal point so a round-trip
    // re-lexes as FLOAT, not INTEGER.
    assert_eq!(cypher_literal(&CypherValue::Float(1.0)), "1.0");
    assert_eq!(cypher_literal(&CypherValue::Float(-0.0)), "-0.0");
    assert_eq!(cypher_literal(&CypherValue::Float(2.5)), "2.5");
    assert_eq!(cypher_literal(&CypherValue::Float(100.0)), "100.0");
    // Contrast: an integer stays an integer.
    assert_eq!(cypher_literal(&CypherValue::Int(1)), "1");

    // Real round-trip through the lexer: `1.0` is a FLOAT token, `1` isn't.
    let float_lit = cypher_literal(&CypherValue::Float(1.0));
    let toks = relate_parser::lexer::tokenize(&float_lit);
    assert_eq!(toks[0].ty, relate_parser::TokenType::Float);
    let int_lit = cypher_literal(&CypherValue::Int(1));
    let itoks = relate_parser::lexer::tokenize(&int_lit);
    assert_eq!(itoks[0].ty, relate_parser::TokenType::Integer);

    // And through the full parser it is a Float literal, not an Integer.
    let q = "RETURN 1.0 AS x";
    let parsed = relate_parser::parse(q).unwrap();
    match &parsed.clauses[..] {
        [Clause::Return(ret)] => match &ret.items[0].expr {
            Expr::Literal(LiteralValue::Float(f)) => {
                assert_eq!(*f, 1.0, "{q}");
            }
            other => panic!("{q}: expected a float literal, got {other:?}"),
        },
        other => panic!("{q}: expected a single RETURN clause, got {other:?}"),
    }
}

#[test]
fn test_string_literal_escapes_backslash_and_quote() {
    // Both backslash and single-quote must be escaped (backslash first).
    let original = "a\\b'c";
    let lit = cypher_literal(&CypherValue::Str(original.into()));
    assert_eq!(lit, "'a\\\\b\\'c'");

    // The escaped literal re-lexes back to the exact original value.
    let toks = relate_parser::lexer::tokenize(&lit);
    assert_eq!(toks[0].ty, relate_parser::TokenType::String);
    assert_eq!(toks[0].literal, original);
}

#[test]
fn test_string_literal_escapes_control_chars() {
    let original = "line1\nline2\ttab\rcr";
    let lit = cypher_literal(&CypherValue::Str(original.into()));
    // Single-line, no raw control bytes in the emitted literal.
    assert!(!lit.contains('\n'));
    assert!(!lit.contains('\t'));
    assert!(!lit.contains('\r'));
    // Round-trips exactly through the lexer.
    let toks = relate_parser::lexer::tokenize(&lit);
    assert_eq!(toks[0].ty, relate_parser::TokenType::String);
    assert_eq!(toks[0].literal, original);
}

#[test]
fn test_cypher_identifier_backticking() {
    // Bare identifiers pass through untouched.
    assert_eq!(cypher_identifier("name"), "name");
    assert_eq!(cypher_identifier("_x1"), "_x1");
    // Anything else gets backticked.
    assert_eq!(cypher_identifier("weird key"), "`weird key`");
    assert_eq!(cypher_identifier("1abc"), "`1abc`"); // digit-leading
    assert_eq!(cypher_identifier(""), "``"); // empty
    assert_eq!(cypher_identifier("caf\u{e9}"), "`caf\u{e9}`"); // non-ASCII
                                                               // Embedded backtick is doubled per Cypher convention.
    assert_eq!(cypher_identifier("a`b"), "`a``b`");

    // A backticked identifier re-lexes as a single Identifier token.
    let quoted = cypher_identifier("weird key");
    let toks = relate_parser::lexer::tokenize(&quoted);
    assert_eq!(toks[0].ty, relate_parser::TokenType::Identifier);
    assert_eq!(toks[0].literal, "weird key");
}

#[test]
fn test_map_key_backticked_in_literal() {
    let mut m = IndexMap::new();
    m.insert("ok".to_string(), CypherValue::Int(1));
    m.insert("needs quote".to_string(), CypherValue::Int(2));
    let lit = cypher_literal(&CypherValue::Map(m));
    assert_eq!(lit, "{ok: 1, `needs quote`: 2}");
    // The whole map literal parses.
    assert!(relate_parser::parse(&format!("RETURN {lit} AS m")).is_ok());
}

#[test]
fn test_nonfinite_float_literals() {
    assert_eq!(cypher_literal(&CypherValue::Float(f64::NAN)), "NaN");
    assert_eq!(
        cypher_literal(&CypherValue::Float(f64::INFINITY)),
        "Infinity"
    );
    assert_eq!(
        cypher_literal(&CypherValue::Float(f64::NEG_INFINITY)),
        "-Infinity"
    );
    // All three are parseable keyword literals, never bare invalid tokens.
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let lit = cypher_literal(&CypherValue::Float(v));
        assert!(
            relate_parser::parse(&format!("RETURN {lit} AS x")).is_ok(),
            "non-finite literal did not parse: {lit}"
        );
    }
}

#[test]
fn test_date_point_temporal_literals_roundtrip() {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    use relate_model::value::{IsoDuration, PointValue};

    let vals = vec![
        CypherValue::Date(NaiveDate::from_ymd_opt(2020, 1, 2).unwrap()),
        CypherValue::Time(NaiveTime::from_hms_opt(12, 34, 56).unwrap(), None),
        CypherValue::LocalDateTime(NaiveDateTime::new(
            NaiveDate::from_ymd_opt(2020, 1, 2).unwrap(),
            NaiveTime::from_hms_opt(12, 34, 56).unwrap(),
        )),
        CypherValue::Duration(IsoDuration::new(14, 3, 3600, 0)),
        CypherValue::Point(PointValue::cartesian_2d(1.0, 2.0)),
        CypherValue::Point(PointValue::cartesian_3d(1.0, 2.0, 3.0)),
    ];
    for v in &vals {
        let lit = cypher_literal(v);
        assert!(
            relate_parser::parse(&format!("RETURN {lit} AS x")).is_ok(),
            "temporal/point literal did not parse: {lit}"
        );
    }
    // Point coords keep their float form.
    assert_eq!(
        cypher_literal(&CypherValue::Point(PointValue::cartesian_2d(1.0, 2.0))),
        "point({srid: 7203, x: 1.0, y: 2.0})"
    );
    assert_eq!(
        cypher_literal(&CypherValue::Date(
            NaiveDate::from_ymd_opt(2020, 1, 2).unwrap()
        )),
        "date('2020-01-02')"
    );
}

#[test]
fn test_list_and_map_literals_parse() {
    let list = CypherValue::List(vec![
        CypherValue::Int(1),
        CypherValue::Float(2.0),
        CypherValue::Str("x'y".into()),
    ]);
    let lit = cypher_literal(&list);
    assert_eq!(lit, "[1, 2.0, 'x\\'y']");
    assert!(relate_parser::parse(&format!("RETURN {lit} AS l")).is_ok());
}

#[test]
fn test_keyword_identifiers_are_backticked() {
    // `Order` lexes as the ORDER keyword. Since ADR-0169 `(n:Order)` parses
    // unquoted, but keywords as variables, aliases and map keys still need
    // backticks, and backticks keep the output readable by older Relate
    // versions that reject keyword labels. The parser's `lookup_keyword`
    // table is the only keyword list; the formatter delegates to it.
    assert_eq!(cypher_identifier("Order"), "`Order`");
    assert_eq!(cypher_identifier("order"), "`order`");
    // Mixed-case non-reserved names stay bare.
    assert_eq!(cypher_identifier("Movie"), "Movie");
    // The backticked form parses as a label.
    let q = format!("CREATE (n:{})", cypher_identifier("Order"));
    assert!(
        relate_parser::parse(&q).is_ok(),
        "backticked reserved-word label must parse: {q}"
    );
}

#[test]
fn test_keyword_map_key_is_backticked_and_round_trips() {
    // ADR-0169 only frees keywords as labels and relationship types; as a map
    // key `order` is still rejected unquoted, so the formatter's backticks are
    // what keep a pulled map literal re-parseable.
    let mut m = IndexMap::new();
    m.insert("order".to_string(), CypherValue::Int(1));
    let lit = cypher_literal(&CypherValue::Map(m));
    assert_eq!(lit, "{`order`: 1}");
    let q = format!("RETURN {lit} AS m");
    let parsed = relate_parser::parse(&q)
        .unwrap_or_else(|e| panic!("backticked reserved-word map key must parse: {q}: {e:?}"));
    // The key reads back as the original name, case preserved.
    match &parsed.clauses[..] {
        [Clause::Return(ret)] => match &ret.items[0].expr {
            Expr::Map(pairs) => {
                assert_eq!(pairs.len(), 1, "{q}");
                assert_eq!(pairs[0].key, "order", "{q}");
            }
            other => panic!("{q}: expected a map literal, got {other:?}"),
        },
        other => panic!("{q}: expected a single RETURN clause, got {other:?}"),
    }
}
