//! `.mode cypher` emission: the scalar-only restriction (8.8) and the
//! LABS-84 hardening (comment/identifier newline break-out, control-char
//! round-trips).

use super::*;

// ---- 8.8: `.mode cypher` scalar-only restriction ----

#[test]
fn test_cypher_mode_scalar_columns_parse() {
    let cols = vec!["name".to_string(), "score".to_string()];
    let s = render(
        Mode::Cypher,
        &cols,
        &[vec![
            CypherValue::Str("Ada".into()),
            CypherValue::Float(1.0),
        ]],
    );
    // Float type preserved and the whole statement re-parses.
    assert!(s.contains("score: 1.0"), "{s}");
    assert!(relate_parser::parse(s.trim()).is_ok(), "unparseable: {s}");
}

#[test]
fn test_cypher_mode_node_column_is_commented_not_garbage() {
    let node = CypherValue::Node {
        id: 0,
        labels: vec!["Person".into()],
        properties: IndexMap::new(),
    };
    let cols = vec!["name".to_string(), "n".to_string()];
    let s = render(
        Mode::Cypher,
        &cols,
        &[vec![CypherValue::Str("Ada".into()), node]],
    );
    // The NODE column is dropped with a clear comment, never emitted as an
    // unparseable `{n: (:Person) [id=0]}`.
    assert!(s.contains("// column n (NODE) omitted"), "{s}");
    assert!(!s.contains("[id=0]"), "leaked node display form: {s}");
    assert!(s.contains("CREATE ({name: 'Ada'})"), "{s}");
    // Every emitted line is valid Cypher (comments included).
    assert!(relate_parser::parse(s.trim()).is_ok(), "unparseable: {s}");
}

#[test]
fn test_cypher_mode_backticks_weird_column_key() {
    let cols = vec!["odd key".to_string()];
    let s = render(Mode::Cypher, &cols, &[vec![CypherValue::Int(7)]]);
    assert!(s.contains("CREATE ({`odd key`: 7})"), "{s}");
    assert!(relate_parser::parse(s.trim()).is_ok(), "unparseable: {s}");
}

// ---- LABS-84: `.mode cypher` hardening ----

#[test]
fn test_cypher_string_literal_escapes_esc_and_roundtrips() {
    let original = "a\u{1b}b";
    let lit = cypher_literal(&CypherValue::Str(original.into()));
    assert!(lit.contains("\\u001B"), "{lit}");
    assert!(!lit.contains('\u{1b}'), "raw ESC leaked: {lit:?}");
    let toks = relate_parser::lexer::tokenize(&lit);
    assert_eq!(toks[0].ty, relate_parser::TokenType::String);
    assert_eq!(toks[0].literal, original);
}

#[test]
fn test_cypher_string_literal_escapes_all_controls() {
    // Every C0 control plus DEL and a C1 char must be neutralized and
    // round-trip through the lexer.
    let original: String = (0u32..=0x1f)
        .chain([0x7f, 0x85])
        .filter_map(char::from_u32)
        .collect();
    let lit = cypher_literal(&CypherValue::Str(original.clone()));
    assert!(
        !lit.chars().any(char::is_control),
        "control leaked: {lit:?}"
    );
    let toks = relate_parser::lexer::tokenize(&lit);
    assert_eq!(toks[0].ty, relate_parser::TokenType::String);
    assert_eq!(toks[0].literal, original);
}

#[test]
fn test_cypher_mode_comment_alias_newline_cannot_break_out() {
    let node = CypherValue::Node {
        id: 0,
        labels: vec!["Person".into()],
        properties: IndexMap::new(),
    };
    let alias = "a\nb";
    let cols = vec![alias.to_string()];
    let s = render(Mode::Cypher, &cols, &[vec![node]]);
    // The comment (plus the empty `CREATE ()`) is all that is emitted; the
    // newline inside the alias must not have produced an extra line.
    assert_eq!(s.lines().count(), 2, "comment broke out: {s:?}");
    assert!(s.contains("a\\u{A}b"), "{s}");
    assert!(s.starts_with("// column "), "{s}");
}

#[test]
fn test_cypher_mode_create_identifier_newline_stays_one_statement() {
    let alias = "c\nx";
    let cols = vec![alias.to_string()];
    let s = render(Mode::Cypher, &cols, &[vec![CypherValue::Int(1)]]);
    assert_eq!(s.lines().count(), 1, "statement split: {s:?}");
    assert!(s.contains("\\u{A}"), "{s}");
    assert!(relate_parser::parse(s.trim()).is_ok(), "unparseable: {s}");
}

#[test]
fn test_map_key_with_control_char_stays_on_one_line() {
    // Map keys ride the same CREATE statement, so they get the same guard.
    let mut m = IndexMap::new();
    m.insert("k\nv".to_string(), CypherValue::Int(1));
    let lit = cypher_literal(&CypherValue::Map(m));
    assert!(!lit.contains('\n'), "raw newline leaked: {lit:?}");
    assert!(lit.contains("\\u{A}"), "{lit}");
}
