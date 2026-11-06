//! Script field values: JSON text to PackStream, the rules an expected
//! value matches an actual one by, and the rendering back to the same
//! notation for failure reports.

use serde_json::{Map, Number, Value};

use super::hex;
use crate::common::packstream::Pack;

/// An expected map value that matches any actual value under its key.
const WILDCARD: &str = "*";

/// Parse the whitespace-separated JSON values after a message name into
/// PackStream fields.
pub(crate) fn parse_fields(text: &str) -> Result<Vec<Pack>, String> {
    let fields = serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("malformed JSON field: {e}"))?;
    check_integer_literals(text)?;
    fields.into_iter().map(from_json).collect()
}

/// Reject an integer literal (no fraction or exponent) outside the i64
/// range. serde_json, built without `arbitrary_precision`, reads a literal
/// above `u64::MAX` as an f64 and drops the token, so this reads the JSON
/// text itself, which serde_json has already validated. Outside string
/// literals, only a number token starts with `-` or a digit.
fn check_integer_literals(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'"' => at = string_end(bytes, at),
            b'-' | b'0'..=b'9' => {
                let len = bytes[at..]
                    .iter()
                    .take_while(|b| matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
                    .count();
                let token = &text[at..at + len];
                if !token.contains(['.', 'e', 'E']) && token.parse::<i64>().is_err() {
                    return Err(format!(
                        "integer {token} does not fit a PackStream Int (i64)"
                    ));
                }
                at += len;
            }
            _ => at += 1,
        }
    }
    Ok(())
}

/// The index just past the string literal whose opening quote is at
/// `start`. UTF-8 continuation bytes are never `"` or `\`, so a byte scan
/// is exact.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut at = start + 1;
    loop {
        match bytes
            .get(at)
            .expect("serde_json validated every string literal as terminated")
        {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
}

/// Map one JSON value to PackStream: an integer is an Int, a number with a
/// fraction or exponent a Float, and `{"#NN": [fields]}` a structure with
/// hex tag NN.
fn from_json(json: Value) -> Result<Pack, String> {
    Ok(match json {
        Value::Null => Pack::Null,
        Value::Bool(b) => Pack::Bool(b),
        Value::Number(n) => number(&n),
        Value::String(s) => Pack::String(s),
        Value::Array(items) => Pack::List(list(items)?),
        Value::Object(entries) => object(entries)?,
    })
}

fn number(n: &Number) -> Pack {
    if let Some(int) = n.as_i64() {
        return Pack::Int(int);
    }
    assert!(
        n.is_f64(),
        "check_integer_literals rejects integer {n} outside i64"
    );
    Pack::Float(n.as_f64().expect("an f64 JSON number has an f64 value"))
}

fn list(items: Vec<Value>) -> Result<Vec<Pack>, String> {
    items.into_iter().map(from_json).collect()
}

/// An object whose only key starts with `#` is a structure; every other
/// object is a map.
fn object(entries: Map<String, Value>) -> Result<Pack, String> {
    if entries.len() == 1 {
        let (key, fields) = entries.into_iter().next().expect("one entry");
        let Some(digits) = key.strip_prefix('#') else {
            return Ok(Pack::Map(vec![(Pack::String(key), from_json(fields)?)]));
        };
        let tag = hex::byte(digits).map_err(|e| format!("bad structure tag `{key}`: {e}"))?;
        let Value::Array(fields) = fields else {
            return Err(format!("structure `{key}` takes a JSON list of fields"));
        };
        return Ok(Pack::Struct {
            tag,
            fields: list(fields)?,
        });
    }
    let entries = entries
        .into_iter()
        .map(|(key, value)| Ok((Pack::String(key), from_json(value)?)))
        .collect::<Result<_, String>>()?;
    Ok(Pack::Map(entries))
}

/// Whether `actual` satisfies `expected`. Structure tags must be equal. An
/// expected map matches when every listed key is present with a matching
/// value — extra actual keys are allowed and the string `"*"` as a map
/// value matches anything. Lists and structure fields match element by
/// element and must have the same length; scalars must be equal.
pub(crate) fn matches(expected: &Pack, actual: &Pack) -> bool {
    match (expected, actual) {
        (Pack::Map(want), Pack::Map(got)) => want.iter().all(|(key, value)| {
            got.iter()
                .find(|(k, _)| k == key)
                .is_some_and(|(_, v)| value.as_str() == Some(WILDCARD) || matches(value, v))
        }),
        (Pack::List(want), Pack::List(got)) => all_match(want, got),
        (
            Pack::Struct { tag, fields },
            Pack::Struct {
                tag: got_tag,
                fields: got_fields,
            },
        ) => tag == got_tag && all_match(fields, got_fields),
        _ => expected == actual,
    }
}

fn all_match(want: &[Pack], got: &[Pack]) -> bool {
    want.len() == got.len() && want.iter().zip(got).all(|(w, g)| matches(w, g))
}

/// Render a value in the script notation.
pub(crate) fn render(value: &Pack) -> String {
    match value {
        Pack::Null => "null".to_string(),
        Pack::Bool(b) => b.to_string(),
        Pack::Int(i) => i.to_string(),
        // Debug keeps a fraction or exponent (`1.0`, `1e300`), so the
        // rendering reads back as a Float.
        Pack::Float(f) => format!("{f:?}"),
        Pack::String(s) => serde_json::to_string(s).expect("a string serializes to JSON"),
        Pack::List(items) => render_list(items),
        Pack::Map(entries) => {
            let entries: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{}: {}", render(key), render(value)))
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
        Pack::Struct { tag, fields } => format!("{{\"#{tag:02X}\": {}}}", render_list(fields)),
        // JSON has no byte strings: display only.
        Pack::Bytes(bytes) => format!("<bytes {}>", hex::encode(bytes)),
    }
}

fn render_list(items: &[Pack]) -> String {
    let items: Vec<String> = items.iter().map(render).collect();
    format!("[{}]", items.join(", "))
}

#[cfg(test)]
#[path = "value_tests.rs"]
mod tests;
