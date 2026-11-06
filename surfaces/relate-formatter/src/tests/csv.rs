//! CSV concerns: header quoting (8.12), formula-injection guard
//! (LABS-84), and control-character escaping (LABS-84).

use super::*;

// ---- 8.12: CSV header escaping ----

#[test]
fn test_csv_header_comma_is_quoted() {
    let cols = vec!["a,b".to_string(), "c".to_string()];
    let s = render(
        Mode::Csv,
        &cols,
        &[vec![CypherValue::Int(1), CypherValue::Int(2)]],
    );
    let mut lines = s.lines();
    assert_eq!(lines.next().unwrap(), "\"a,b\",c");
    assert_eq!(lines.next().unwrap(), "1,2");
}

#[test]
fn test_csv_header_quote_is_escaped() {
    let cols = vec!["say \"hi\"".to_string()];
    let s = render(Mode::Csv, &cols, &[vec![CypherValue::Int(1)]]);
    assert_eq!(s.lines().next().unwrap(), "\"say \"\"hi\"\"\"");
}

// ---- LABS-84: CSV formula injection ----

#[test]
fn test_csv_formula_guard_equals() {
    assert_eq!(csv_value_line("=1+1"), "'=1+1");
}

#[test]
fn test_csv_formula_guard_plus() {
    assert_eq!(csv_value_line("+1"), "'+1");
}

#[test]
fn test_csv_formula_guard_minus() {
    assert_eq!(csv_value_line("-1"), "'-1");
}

#[test]
fn test_csv_formula_guard_at() {
    assert_eq!(csv_value_line("@SUM(A1)"), "'@SUM(A1)");
}

#[test]
fn test_csv_formula_guard_tab() {
    // The leading tab is escaped to the literal `\t` before the formula guard
    // runs, so the field no longer starts with a control byte and gets no `'`
    // prefix.
    assert_eq!(csv_value_line("\tcmd"), "\\tcmd");
}

#[test]
fn test_csv_formula_guard_cr() {
    // CR triggers both the formula guard and RFC-4180 quoting.
    assert_eq!(csv_value_line("\rcmd"), "\"'\rcmd\"");
}

#[test]
fn test_csv_formula_guard_header() {
    assert_eq!(csv_header_line("=cmd"), "'=cmd");
}

#[test]
fn test_csv_normal_value_unchanged() {
    assert_eq!(csv_value_line("Alice"), "Alice");
    assert_eq!(csv_header_line("name"), "name");
}

// ---- LABS-84: CSV control-character escaping ----

#[test]
fn test_csv_escapes_control_chars() {
    // Cc controls (ESC, BEL, NUL) and Cf format chars (U+202E) are escaped to
    // the braced uppercase form; the escape is plain text, so it is unquoted.
    assert_eq!(csv_value_line("a\u{1b}b"), "a\\u{1B}b");
    assert_eq!(csv_value_line("\u{7}"), "\\u{7}");
    assert_eq!(csv_value_line("\u{0}"), "\\u{0}");
    assert_eq!(csv_value_line("\u{202e}"), "\\u{202E}");
    // No raw control byte survives.
    for c in ['\u{1b}', '\u{7}', '\u{0}', '\u{202e}'] {
        assert!(
            !csv_value_line(&format!("a{c}b")).contains(c),
            "raw {c:?} leaked"
        );
    }
}

#[test]
fn test_csv_keeps_raw_lf_and_cr_inside_quotes() {
    // LF and CR are structural for RFC 4180: they stay raw and force quoting,
    // so a multi-line value keeps its exact bytes.
    let cols = vec!["v".to_string()];
    let s = render(
        Mode::Csv,
        &cols,
        &[
            vec![CypherValue::Str("a\nb".into())],
            vec![CypherValue::Str("a\rb".into())],
        ],
    );
    assert!(s.contains("\"a\nb\""), "{s:?}");
    assert!(s.contains("\"a\rb\""), "{s:?}");
    // The newline/CR were not rewritten to their `\n`/`\r` escapes.
    assert!(!s.contains("\\n"), "{s:?}");
    assert!(!s.contains("\\r"), "{s:?}");
}

#[test]
fn test_csv_backslash_passthrough() {
    // CSV data is not byte-exact, so a backslash is emitted unchanged rather
    // than doubled.
    assert_eq!(csv_value_line("a\\b"), "a\\b");
}

#[test]
fn test_csv_header_escapes_control_chars() {
    assert_eq!(csv_header_line("a\u{1b}b"), "a\\u{1B}b");
    assert_eq!(csv_header_line("\u{202e}x"), "\\u{202E}x");
}
