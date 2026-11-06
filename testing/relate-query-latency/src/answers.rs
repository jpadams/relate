//! Compare a query's rows with its expected answer, by upstream's rules
//! (`neo4j/benchmark_query.py` `_assert_rows` / `_assert_single_value`)
//! without type coercion (ADR-0243).
//!
//! - order-sensitive: list equality, row by row;
//! - otherwise: multiset equality of each row's sorted `(column, value)`
//!   pairs.
//!
//! Values compare by type and value: an INTEGER never equals a BOOLEAN or a
//! FLOAT (upstream's Python `==` lets `True == 1` and `1 == 1.0` pass), and
//! column names must match exactly.

use relate::Value;

/// One query's expected answer — the rows shape every lane's oracle
/// records (`graph-benchmark-ldbc`'s `expected-sf<SF>.json`, the fraud workload's
/// `oracle.json` rows checks) and [`compare`] judges, so the type and
/// its operators share this one lane-neutral home.
#[derive(Debug, Clone)]
pub struct Expected {
    /// Rows as column name → JSON value.
    pub rows: Vec<serde_json::Map<String, serde_json::Value>>,
    /// List equality when true, multiset equality otherwise.
    pub order_sensitive: bool,
}

/// A value reduced to what the comparison sees. `Float` holds the bit
/// pattern so the type is totally ordered; the suite's answers hold no NaN.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Norm {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(String),
    List(Vec<Norm>),
    Map(Vec<(String, Norm)>),
    /// A value the JSON answers cannot hold (node, temporal, …), by its
    /// debug form; it never equals an expected value.
    Other(String),
}

/// A row as sorted `(column, value)` pairs.
pub type Row = Vec<(String, Norm)>;

impl Norm {
    pub fn from_engine(v: &Value) -> Self {
        match v {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Int(i) => Self::Int(*i),
            Value::Float(f) => Self::Float(f.to_bits()),
            Value::String(s) => Self::Str(s.to_string()),
            Value::List(items) => Self::List(items.iter().map(Self::from_engine).collect()),
            Value::Map(m) => Self::Map(
                m.iter()
                    .map(|(k, v)| (k.clone(), Self::from_engine(v)))
                    .collect(),
            ),
            other => Self::Other(format!("{other:?}")),
        }
    }

    pub fn from_json(v: &serde_json::Value) -> Self {
        use serde_json::Value as J;
        match v {
            J::Null => Self::Null,
            J::Bool(b) => Self::Bool(*b),
            J::Number(n) => match n.as_i64() {
                Some(i) => Self::Int(i),
                None => Self::Float(n.as_f64().unwrap_or(f64::NAN).to_bits()),
            },
            J::String(s) => Self::Str(s.clone()),
            J::Array(items) => Self::List(items.iter().map(Self::from_json).collect()),
            J::Object(m) => {
                let mut pairs: Vec<_> = m
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::from_json(v)))
                    .collect();
                pairs.sort();
                Self::Map(pairs)
            }
        }
    }

    /// The value as JSON, for reports and diffs.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Self::Null => J::Null,
            Self::Bool(b) => J::Bool(*b),
            Self::Int(i) => J::from(*i),
            Self::Float(bits) => J::from(f64::from_bits(*bits)),
            Self::Str(s) => J::String(s.clone()),
            Self::List(items) => J::Array(items.iter().map(Self::to_json).collect()),
            Self::Map(pairs) => J::Object(
                pairs
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_json()))
                    .collect(),
            ),
            Self::Other(s) => J::String(format!("<{s}>")),
        }
    }
}

/// One executed result's answer check against its expected rows — the
/// difference when the answer does not match, `None` when it does. The one
/// composition every answer-checking site runs (the graph-benchmark-ldbc `run` and `profile`
/// lanes, the fraud and ukrailway corpus lanes), so no site re-spells the
/// engine-rows-then-compare pair. Its consumers are all in-crate; the
/// module's outside-crate surface is [`compare`] and its row types.
pub(crate) fn diff(result: &relate::QueryResult, expected: &Expected) -> Option<String> {
    compare(&engine_rows(&result.header.columns, &result.rows), expected)
}

/// The engine's rows as comparison rows.
pub fn engine_rows(columns: &[String], rows: &[Vec<Value>]) -> Vec<Row> {
    rows.iter()
        .map(|row| {
            let mut pairs: Row = columns
                .iter()
                .cloned()
                .zip(row.iter().map(Norm::from_engine))
                .collect();
            pairs.sort();
            pairs
        })
        .collect()
}

fn expected_rows(exp: &Expected) -> Vec<Row> {
    exp.rows
        .iter()
        .map(|r| {
            let mut pairs: Row = r
                .iter()
                .map(|(k, v)| (k.clone(), Norm::from_json(v)))
                .collect();
            pairs.sort();
            pairs
        })
        .collect()
}

/// A row as a JSON object, for diffs.
pub fn row_json(row: &Row) -> serde_json::Value {
    serde_json::Value::Object(row.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
}

/// `None` when `got` equals the expected answer, else a short description
/// of the first difference.
pub fn compare(got: &[Row], exp: &Expected) -> Option<String> {
    const SHOW: usize = 5;
    let want = expected_rows(exp);
    if exp.order_sensitive {
        if got == want.as_slice() {
            return None;
        }
        let first = got
            .iter()
            .zip(&want)
            .position(|(a, b)| a != b)
            .unwrap_or(got.len().min(want.len()));
        let show = |rows: &[Row]| {
            rows.get(first)
                .map(row_json)
                .unwrap_or(serde_json::Value::Null)
        };
        return Some(format!(
            "order-sensitive: got {} rows, expected {}; first difference at row {first}: got {}, expected {}",
            got.len(),
            want.len(),
            show(got),
            show(&want)
        ));
    }
    let mut a = got.to_vec();
    let mut b = want;
    a.sort();
    b.sort();
    if a == b {
        return None;
    }
    let (missing, unexpected) = multiset_difference(&a, &b);
    let list = |rows: &[&Row]| {
        rows.iter()
            .take(SHOW)
            .map(|r| row_json(r).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Some(format!(
        "multiset: got {} rows, expected {}; missing {}: [{}]; unexpected {}: [{}]",
        a.len(),
        b.len(),
        missing.len(),
        list(&missing),
        unexpected.len(),
        list(&unexpected)
    ))
}

/// Rows of sorted `want` absent from sorted `got`, and the reverse, with
/// multiplicity.
fn multiset_difference<'a>(got: &'a [Row], want: &'a [Row]) -> (Vec<&'a Row>, Vec<&'a Row>) {
    let (mut i, mut j) = (0, 0);
    let (mut missing, mut unexpected) = (Vec::new(), Vec::new());
    while i < got.len() || j < want.len() {
        match (got.get(i), want.get(j)) {
            (Some(g), Some(w)) if g == w => {
                i += 1;
                j += 1;
            }
            (Some(g), Some(w)) if g < w => {
                unexpected.push(g);
                i += 1;
            }
            (_, Some(w)) => {
                missing.push(w);
                j += 1;
            }
            (Some(g), None) => {
                unexpected.push(g);
                i += 1;
            }
            (None, None) => unreachable!(),
        }
    }
    (missing, unexpected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workload::parse_expected;

    fn exp(json: &str) -> Expected {
        parse_expected(&format!(r#"{{"q1": {json}}}"#))
            .unwrap()
            .remove(&1)
            .unwrap()
    }

    fn rows(columns: &[&str], rows: Vec<Vec<Value>>) -> Vec<Row> {
        let cols: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
        engine_rows(&cols, &rows)
    }

    #[test]
    fn integer_never_equals_bool_or_float() {
        let e = exp(r#"{"rows": [{"x": 1}], "order_sensitive": true}"#);
        assert!(compare(&rows(&["x"], vec![vec![Value::Int(1)]]), &e).is_none());
        assert!(compare(&rows(&["x"], vec![vec![Value::Bool(true)]]), &e).is_some());
        assert!(compare(&rows(&["x"], vec![vec![Value::Float(1.0)]]), &e).is_some());
        let f = exp(r#"{"rows": [{"x": 1.0}], "order_sensitive": true}"#);
        assert!(compare(&rows(&["x"], vec![vec![Value::Float(1.0)]]), &f).is_none());
        assert!(compare(&rows(&["x"], vec![vec![Value::Int(1)]]), &f).is_some());
    }

    #[test]
    fn column_names_must_match() {
        let e = exp(r#"{"rows": [{"p.firstName": "Ann"}], "order_sensitive": true}"#);
        assert!(compare(
            &rows(&["firstName"], vec![vec![Value::String("Ann".into())]]),
            &e
        )
        .is_some());
        assert!(compare(
            &rows(&["p.firstName"], vec![vec![Value::String("Ann".into())]]),
            &e
        )
        .is_none());
    }

    #[test]
    fn order_sensitive_answers_compare_as_lists() {
        let e = exp(r#"{"rows": [{"x": 1}, {"x": 2}], "order_sensitive": true}"#);
        let swapped = rows(&["x"], vec![vec![Value::Int(2)], vec![Value::Int(1)]]);
        let diff = compare(&swapped, &e).unwrap();
        assert!(diff.contains("first difference at row 0"), "{diff}");
    }

    #[test]
    fn unordered_answers_compare_as_multisets() {
        let e = exp(r#"{"rows": [{"x": 1}, {"x": 2}, {"x": 2}], "order_sensitive": false}"#);
        let shuffled = rows(
            &["x"],
            vec![
                vec![Value::Int(2)],
                vec![Value::Int(1)],
                vec![Value::Int(2)],
            ],
        );
        assert!(compare(&shuffled, &e).is_none());
        let short = rows(
            &["x"],
            vec![
                vec![Value::Int(2)],
                vec![Value::Int(1)],
                vec![Value::Int(3)],
            ],
        );
        let diff = compare(&short, &e).unwrap();
        assert!(diff.contains(r#"missing 1: [{"x":2}]"#), "{diff}");
        assert!(diff.contains(r#"unexpected 1: [{"x":3}]"#), "{diff}");
    }

    #[test]
    fn empty_answers_match_only_empty_results() {
        let e = exp(r#"{"rows": [], "order_sensitive": false}"#);
        assert!(compare(&[], &e).is_none());
        assert!(compare(&rows(&["x"], vec![vec![Value::Null]]), &e).is_some());
    }

    #[test]
    fn nested_values_compare_structurally() {
        let e =
            exp(r#"{"rows": [{"l": [1, "a", null], "m": {"k": false}}], "order_sensitive": true}"#);
        let m = std::collections::BTreeMap::from([("k".to_string(), Value::Bool(false))]);
        let got = rows(
            &["l", "m"],
            vec![vec![
                Value::List(vec![Value::Int(1), Value::String("a".into()), Value::Null].into()),
                Value::Map(m.into()),
            ]],
        );
        assert!(compare(&got, &e).is_none());
    }

    /// The one composition every answer-checking site runs: a real executed
    /// result against its expected rows — matching rows diff to `None`, a
    /// mismatch carries the difference.
    #[test]
    fn diff_checks_one_executed_result() {
        let db = relate::Database::open(":memory:").unwrap();
        let mut conn = db.connect().unwrap();
        let result = conn.query("UNWIND [1, 2] AS x RETURN x").unwrap();
        let e = exp(r#"{"rows": [{"x": 1}, {"x": 2}], "order_sensitive": false}"#);
        assert!(diff(&result, &e).is_none());
        let e = exp(r#"{"rows": [{"x": 1}], "order_sensitive": false}"#);
        let d = diff(&result, &e).unwrap();
        assert!(d.contains("unexpected 1"), "{d}");
    }
}
