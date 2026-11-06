//! The typed row decoders: every cell the verbs read off a result row
//! goes through one of these, and a malformed row fails the turn loudly
//! — never a silent default the player reads as game content.

use crate::data::Direction;
use serde_json::Value;

/// One string cell off a result row. A missing or non-string cell is a
/// typed error — a malformed row must fail the turn loudly, never
/// surface as a default the player reads as content.
pub(crate) fn cell<'v>(row: &'v Value, key: &str) -> Result<&'v str, String> {
    row.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("the row answered no string '{key}': {row}"))
}

/// One integer cell, same policy.
pub(crate) fn int_cell(row: &Value, key: &str) -> Result<i64, String> {
    row.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("the row answered no integer '{key}': {row}"))
}

/// One boolean cell, same policy.
pub(crate) fn bool_cell(row: &Value, key: &str) -> Result<bool, String> {
    row.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("the row answered no boolean '{key}': {row}"))
}

/// One compass rel type off a row — an off-vocabulary type is an error,
/// never a silently dropped passage.
pub(crate) fn direction_cell(row: &Value, key: &str) -> Result<Direction, String> {
    let text = cell(row, key)?;
    Direction::from_rel_type(text)
        .ok_or_else(|| format!("an off-vocabulary rel type '{text}': {row}"))
}

/// One `(x, y)` coordinate pair off a row, the loud i32 bound included
/// (a coordinate out of range wraps to a wrong grid position if
/// truncated silently).
pub(crate) fn coords(row: &Value, kx: &str, ky: &str) -> Result<(i32, i32), String> {
    let x = int_cell(row, kx)?;
    let y = int_cell(row, ky)?;
    let x = i32::try_from(x).map_err(|_| format!("a coordinate out of i32 range: {x}"))?;
    let y = i32::try_from(y).map_err(|_| format!("a coordinate out of i32 range: {y}"))?;
    Ok((x, y))
}
