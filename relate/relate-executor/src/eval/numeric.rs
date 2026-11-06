//! Numeric function helpers (`round`, unary int/float adapters).

use relate_core::{ArithmeticFailure, Error, Result, Value};

/// The largest `round()` precision the f64-backed engine supports: an f64
/// carries at most ~15–17 significant decimal digits, so a larger decimal
/// precision cannot express anything the value does not already have, and
/// the scale computation would overflow (ADR-0336).
const MAX_ROUND_PRECISION: i64 = 15;

/// `2^52`: the first f64 magnitude whose spacing exceeds 1, i.e. the point
/// past which no value has a fractional part to round. `round()` is the
/// identity from there on (TCK RoundingNumbers [4], [5]).
const NO_FRACTION_LIMIT: f64 = 4_503_599_627_370_496.0;

/// The `round()` mode vocabulary (Neo4j's eight names), parsed once at the
/// call boundary so the rounding dispatch matches on a typed mode instead of
/// re-classifying the raw strings (the `TruncUnit::parse` shape in
/// relate-core).
#[derive(Clone, Copy)]
enum RoundingMode {
    Up,
    Down,
    Ceiling,
    Floor,
    /// The wire spelling `HALF_UP`: ties away from zero (Java's
    /// `RoundingMode.HALF_UP`).
    HalfUp,
    HalfDown,
    HalfEven,
    Unnecessary,
    /// The mode of a `round()` call that names none: ties toward positive
    /// infinity (`floor(x + 0.5)`), which is *not* the wire `HALF_UP` —
    /// `round(-1.5)` is -1.0 while `round(-1.5, 0, 'HALF_UP')` is -2.0.
    HalfUpTowardPositiveInfinity,
}

impl RoundingMode {
    /// Parses a wire spelling; any other spelling is the
    /// `UnsupportedRoundingMode` error.
    fn parse(mode: &str) -> Result<RoundingMode> {
        Ok(match mode {
            "UP" => RoundingMode::Up,
            "DOWN" => RoundingMode::Down,
            "CEILING" => RoundingMode::Ceiling,
            "FLOOR" => RoundingMode::Floor,
            "HALF_UP" => RoundingMode::HalfUp,
            "HALF_DOWN" => RoundingMode::HalfDown,
            "HALF_EVEN" => RoundingMode::HalfEven,
            "UNNECESSARY" => RoundingMode::Unnecessary,
            _ => return Err(Error::execution("UnsupportedRoundingMode")),
        })
    }
}

/// The `round()` precision argument, normalized once: an INTEGER and a FLOAT
/// spelling both flow through [`Value::as_number`], so one range-check pair
/// decides `-1` and `-1.0` alike (both a range rejection, not a type one).
/// A fraction truncates toward zero — the reference engine's
/// `round(3.14…, -0.02)` is `round(3.14…, 0)` — and a value that is not a
/// truncatable number keeps the `must be INTEGER` type rejection.
fn round_precision(v: &Value) -> Result<i32> {
    let Some(n) = v.as_number() else {
        return Err(Error::invalid_argument_type(
            "round precision must be INTEGER",
        ));
    };
    if !n.is_finite() {
        return Err(Error::invalid_argument_type(
            "round precision must be INTEGER",
        ));
    }
    let n = n.trunc();
    if n < 0.0 {
        return Err(Error::execution(
            "NumericValueOutOfRange: round precision must be non-negative",
        ));
    }
    if n > MAX_ROUND_PRECISION as f64 {
        return Err(Error::execution(format!(
            "NumericValueOutOfRange: round precision must be at most {MAX_ROUND_PRECISION}"
        )));
    }
    Ok(n as i32)
}

pub(crate) fn round_function(vals: &[Value]) -> Result<Value> {
    if vals.is_empty() || vals.len() > 3 {
        return Err(Error::syntax(
            "InvalidNumberOfArguments: round() requires one to three arguments",
        ));
    }
    if vals.iter().any(Value::is_null) {
        return Ok(Value::Null);
    }
    let value = vals[0]
        .as_number()
        .ok_or_else(|| Error::invalid_argument_type("round value must be numeric"))?;
    let precision = match vals.get(1) {
        None => 0,
        Some(v) => round_precision(v)?,
    };
    let mode = match vals.get(2) {
        None => RoundingMode::HalfUpTowardPositiveInfinity,
        Some(Value::String(mode)) => RoundingMode::parse(mode)?,
        _ => return Err(Error::invalid_argument_type("round mode must be STRING")),
    };
    if !value.is_finite() {
        return Ok(Value::Float(value));
    }
    // At or beyond 2^52 an f64 has no fractional part left to round, and
    // both scaling by 10^precision and `x + 0.5` perturb the value (the
    // former injects ulp noise, the latter can step to the next float), so
    // every mode is the identity there.
    if value.abs() >= NO_FRACTION_LIMIT {
        return Ok(Value::Float(value));
    }
    let scale = 10_f64.powi(precision);
    let scaled = value * scale;
    let rounded = match mode {
        RoundingMode::Up => {
            if scaled.is_sign_negative() {
                scaled.floor()
            } else {
                scaled.ceil()
            }
        }
        RoundingMode::Down => scaled.trunc(),
        RoundingMode::Ceiling => scaled.ceil(),
        RoundingMode::Floor => scaled.floor(),
        RoundingMode::HalfUp => scaled.round(),
        RoundingMode::HalfUpTowardPositiveInfinity => (scaled + 0.5).floor(),
        RoundingMode::HalfEven => scaled.round_ties_even(),
        RoundingMode::HalfDown => {
            let trunc = scaled.trunc();
            if (scaled - trunc).abs() > 0.5 {
                scaled.round()
            } else {
                trunc
            }
        }
        RoundingMode::Unnecessary if scaled.fract() != 0.0 => {
            return Err(Error::arithmetic(ArithmeticFailure::RoundingNecessary))
        }
        RoundingMode::Unnecessary => scaled,
    };
    Ok(Value::Float(rounded / scale))
}

/// Generic numeric unary op. The integer arm must be a checked op (`None`
/// on overflow — e.g. `abs(i64::MIN)`) so extreme integers surface an honest
/// range error instead of unwinding into the host.
pub(crate) fn num_unary(
    vals: &[Value],
    on_int: impl Fn(i64) -> Option<i64>,
    on_float: impl Fn(f64) -> f64,
) -> Result<Value> {
    Ok(match vals.first() {
        Some(Value::Int(i)) => {
            Value::Int(on_int(*i).ok_or_else(|| Error::ty("integer out of range"))?)
        }
        Some(Value::Float(f)) => Value::Float(on_float(*f)),
        Some(Value::Null) | None => Value::Null,
        _ => return Err(Error::ty("expected number")),
    })
}

pub(crate) fn float_unary(vals: &[Value], f: impl Fn(f64) -> f64) -> Result<Value> {
    Ok(match vals.first() {
        Some(Value::Int(i)) => Value::Float(f(*i as f64)),
        Some(Value::Float(x)) => Value::Float(f(*x)),
        Some(Value::Null) | None => Value::Null,
        _ => return Err(Error::ty("expected number")),
    })
}

/// The shared FLOAT → INTEGER conversion (ADR-0168, ADR-0175), used by the
/// integer-argument sites (`range()`, `substring()` / `left()` /
/// `right()`, list-slice bounds) and the `toInteger()` /
/// `toIntegerOrNull()` / `toIntegerList()` family. It is Java's
/// `(long) d`, which Neo4j applies through `NumberValue.longValue()`, and
/// Rust's saturating `as` cast has exactly those semantics: NaN converts
/// to 0, values at or beyond the `i64` limits (±Infinity included)
/// saturate to those limits, and every other value truncates toward zero.
/// A float never fails to convert.
pub(crate) fn float_to_i64(f: f64) -> i64 {
    f as i64
}

/// ADR-0175's shared integer-argument coercion (`range()`,
/// `substring()` / `left()` / `right()`, list-slice bounds): an INTEGER
/// passes through, a FLOAT converts via [`float_to_i64`], and anything
/// else — null, missing, or non-numeric — is `None` so the call site
/// applies its own policy (type error, range gate, or default fallback).
pub(crate) fn int_arg(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Int(i)) => Some(*i),
        Some(Value::Float(f)) => Some(float_to_i64(*f)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0336: the precision is bounded to 0..=15 — a larger one gets the
    /// same NumericValueOutOfRange treatment as a negative one, never the
    /// NaN the overflow-prone scale computation used to produce.
    #[test]
    fn round_precision_is_bounded() {
        let too_large =
            Error::execution("NumericValueOutOfRange: round precision must be at most 15");
        for precision in [16, 400, i64::MAX] {
            assert_eq!(
                round_function(&[Value::Float(2.5), Value::Int(precision)]).unwrap_err(),
                too_large,
                "precision {precision}"
            );
        }
        // One normalized range check: the INTEGER and FLOAT spellings share
        // both the negative and the upper-bound class (`round(x, -1)` and
        // `round(x, -1.0)` used to diverge into "non-negative" vs "must be
        // INTEGER").
        let negative =
            Error::execution("NumericValueOutOfRange: round precision must be non-negative");
        for precision in [Value::Int(-1), Value::Float(-1.0), Value::Float(-1.5)] {
            assert_eq!(
                round_function(&[Value::Float(2.5), precision.clone()]).unwrap_err(),
                negative,
                "precision {precision:?}"
            );
        }
        for precision in [Value::Int(16), Value::Float(16.0)] {
            assert_eq!(
                round_function(&[Value::Float(2.5), precision.clone()]).unwrap_err(),
                too_large,
                "precision {precision:?}"
            );
        }
        assert_eq!(
            round_function(&[Value::Float(2.5), Value::Int(15)]).unwrap(),
            Value::Float(2.5)
        );
        assert_eq!(
            round_function(&[Value::Float(2.5)]).unwrap(),
            Value::Float(3.0)
        );
        // The implicit form is HALF_UP toward positive infinity, not the
        // wire `HALF_UP` (away from zero): -2.5 rounds to -2.0.
        assert_eq!(
            round_function(&[Value::Float(-2.5)]).unwrap(),
            Value::Float(-2.0)
        );
        assert_eq!(
            round_function(&[Value::Float(2.5), Value::Int(2)]).unwrap(),
            Value::Float(2.5)
        );
        assert_eq!(
            round_function(&[Value::Null, Value::Int(400)]).unwrap(),
            Value::Null
        );
    }

    /// A float precision is accepted and truncated, matching Neo4j:
    /// `round(1.23456, 2.5)` is 1.23, and a non-finite value stays itself.
    #[test]
    fn round_accepts_a_float_precision_by_truncation() {
        assert_eq!(
            round_function(&[Value::Float(1.23456), Value::Float(2.5)]).unwrap(),
            Value::Float(1.23)
        );
        assert_eq!(
            round_function(&[Value::Float(f64::INFINITY), Value::Float(0.0)]).unwrap(),
            Value::Float(f64::INFINITY)
        );
        // Truncation is toward zero, so a negative fraction below one is
        // precision 0: the reference engine's `round(x, -0.02)` is
        // `round(x, 0)`.
        assert_eq!(
            round_function(&[Value::Float(3.7), Value::Float(-0.02)]).unwrap(),
            Value::Float(4.0)
        );
        assert!(round_function(&[Value::Float(1.23), Value::Float(-1.0)]).is_err());
    }

    /// The `UNNECESSARY` mode keeps its typed failure at a bounded
    /// precision.
    #[test]
    fn round_unnecessary_keeps_its_typed_failure() {
        assert_eq!(
            round_function(&[
                Value::Float(2.5),
                Value::Int(0),
                Value::String("UNNECESSARY".into())
            ])
            .unwrap_err(),
            Error::arithmetic(ArithmeticFailure::RoundingNecessary)
        );
        assert_eq!(
            round_function(&[
                Value::Float(2.0),
                Value::Int(0),
                Value::String("UNNECESSARY".into())
            ])
            .unwrap(),
            Value::Float(2.0)
        );
    }

    /// The mode vocabulary parses once at the boundary: every accepted
    /// spelling reaches a typed variant, and any other spelling keeps the
    /// `UnsupportedRoundingMode` error.
    #[test]
    fn round_mode_parses_the_wire_vocabulary_once() {
        for mode in [
            "UP",
            "DOWN",
            "CEILING",
            "FLOOR",
            "HALF_UP",
            "HALF_DOWN",
            "HALF_EVEN",
            "UNNECESSARY",
        ] {
            assert!(RoundingMode::parse(mode).is_ok(), "{mode}");
        }
        assert_eq!(
            round_function(&[
                Value::Float(2.5),
                Value::Int(0),
                Value::String("half_up".into())
            ])
            .unwrap_err(),
            Error::execution("UnsupportedRoundingMode")
        );
    }
}
