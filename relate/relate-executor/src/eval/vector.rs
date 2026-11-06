//! Vector value helpers: construction, arithmetic, norms, and distances.

use relate_core::{Error, Result, Value, VectorCoordKind};
use relate_ir::VectorMetric;

pub(crate) fn as_floats(v: &Value) -> Option<Vec<f64>> {
    match v {
        Value::Vector { values, .. } => values
            .iter()
            .map(|x| match x {
                Value::Int(i) => Some(*i as f64),
                Value::Float(f) => Some(*f),
                _ => None,
            })
            .collect(),
        Value::List(xs) => xs
            .iter()
            .map(|x| match x {
                Value::Int(i) => Some(*i as f64),
                Value::Float(f) => Some(*f),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

pub(crate) fn vector_construct(vals: &[Value]) -> Result<Value> {
    if vals.len() != 3 {
        return Err(Error::semantic("vector() requires three arguments"));
    }
    if vals.iter().any(Value::is_null) {
        return Ok(Value::Null);
    }
    let parsed;
    let items: &[Value] = match &vals[0] {
        Value::List(items) => items,
        Value::String(s) => {
            let body = s
                .trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .ok_or_else(|| Error::ty("InvalidVectorCoordinates"))?;
            parsed = if body.trim().is_empty() {
                Vec::new()
            } else {
                body.split(',')
                    .map(|token| {
                        token
                            .trim()
                            .replace('_', "")
                            .parse::<f64>()
                            .map(Value::Float)
                            .map_err(|_| Error::ty("InvalidVectorCoordinates"))
                    })
                    .collect::<Result<Vec<_>>>()?
            };
            &parsed
        }
        _ => return Err(Error::ty("vector() requires a numeric list")),
    };
    let dim = vals[1]
        .as_int()
        .ok_or_else(|| Error::ty("vector() dimension must be an integer"))?;
    if dim < 0 || dim as usize != items.len() {
        return Err(Error::ty("vector() dimension does not match list"));
    }
    let coord_name = vals[2]
        .as_string()
        .ok_or_else(|| Error::ty("vector() coordinate type must be a keyword"))?
        .to_ascii_uppercase();
    let coord = VectorCoordKind::from_alias(&coord_name)
        .ok_or_else(|| Error::ty("unknown vector coordinate type"))?;
    // Choose the coordinate conversion once: it depends only on the
    // loop-invariant `coord`, not on the element being converted.
    let convert = coord_converter(coord);
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let n = item
            .as_number()
            .ok_or_else(|| Error::ty("vector() coordinates must be numbers"))?;
        values.push(convert(n)?);
    }
    Ok(Value::Vector {
        coord,
        values: values.into(),
    })
}

/// The per-element coordinate conversion for one `VectorCoordKind`, chosen
/// once per `vector()` call so the loop body never re-dispatches on the
/// loop-invariant kind.
fn coord_converter(coord: VectorCoordKind) -> fn(f64) -> Result<Value> {
    match coord {
        VectorCoordKind::Int8 => |n| int_coord(n, i8::MIN as f64, i8::MAX as f64),
        VectorCoordKind::Int16 => |n| int_coord(n, i16::MIN as f64, i16::MAX as f64),
        VectorCoordKind::Int32 => |n| int_coord(n, i32::MIN as f64, i32::MAX as f64),
        VectorCoordKind::Int64 => |n| int_coord(n, i64::MIN as f64, i64::MAX as f64),
        VectorCoordKind::Float16 => float16_coord,
        VectorCoordKind::BFloat16 => bfloat16_coord,
        VectorCoordKind::Float32 => float32_coord,
        VectorCoordKind::Float64 => float64_coord,
    }
}

/// Integer-family conversion: NaN coerces to 0, any other non-finite input
/// is an error, and a finite out-of-range value is an `Overflow`.
fn int_coord(n: f64, min: f64, max: f64) -> Result<Value> {
    let n = if n.is_nan() { 0.0 } else { n };
    if !n.is_finite() {
        return Err(Error::ty("vector() coordinates must be finite numbers"));
    }
    if n < min || n > max {
        return Err(Error::ty("Overflow"));
    }
    Ok(Value::Int(n as i64))
}

/// Float-family conversion: the input must be finite, and the value
/// narrowed to the coordinate kind's precision must stay finite (and thus
/// fit the kind's range). The narrowing itself is the model's one
/// [`VectorCoordKind::narrow_float`].
fn float_coord(n: f64, kind: VectorCoordKind) -> Result<Value> {
    if !n.is_finite() {
        return Err(Error::ty("vector() coordinates must be finite numbers"));
    }
    kind.narrow_float(n)
        .map(Value::Float)
        .ok_or_else(|| Error::ty("InvalidVectorCoordinates"))
}

fn float16_coord(n: f64) -> Result<Value> {
    float_coord(n, VectorCoordKind::Float16)
}

fn bfloat16_coord(n: f64) -> Result<Value> {
    float_coord(n, VectorCoordKind::BFloat16)
}

fn float32_coord(n: f64) -> Result<Value> {
    float_coord(n, VectorCoordKind::Float32)
}

/// Float64 conversion: any finite `f64` is stored as-is.
fn float64_coord(n: f64) -> Result<Value> {
    float_coord(n, VectorCoordKind::Float64)
}

pub(crate) fn vector_dimension_count(vals: &[Value]) -> Result<Value> {
    Ok(match plain_vector(vals.first().unwrap_or(&Value::Null))? {
        Some(xs) => Value::Int(xs.len() as i64),
        None => Value::Null,
    })
}

pub(crate) fn plain_vector(value: &Value) -> Result<Option<Vec<f64>>> {
    match value {
        Value::Null => Ok(None),
        Value::Vector { .. } => Ok(as_floats(value)),
        _ => Err(Error::invalid_argument_type("expected a VECTOR")),
    }
}

pub(crate) fn vector_metric(a: &[f64], b: &[f64], metric: VectorMetric) -> Result<f64> {
    let round = |v: f64| v as f32 as f64;
    Ok(match metric {
        VectorMetric::Euclidean => round(
            a.iter()
                .zip(b)
                .map(|(x, y)| (x - y).powi(2))
                .sum::<f64>()
                .sqrt(),
        ),
        VectorMetric::EuclideanSquared => {
            round(a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum())
        }
        VectorMetric::Manhattan => round(a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum()),
        VectorMetric::Cosine => {
            let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
            let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
            round(1.0 - round(dot / (na * nb)))
        }
        VectorMetric::Dot => round(-a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>()),
        VectorMetric::Hamming => a.iter().zip(b).filter(|(x, y)| x != y).count() as f64,
    })
}

pub(crate) fn vector_distance_metric(vals: &[Value], metric: VectorMetric) -> Result<Value> {
    if vals.len() != 2 {
        return Err(Error::syntax(
            "InvalidNumberOfArguments: vector_distance() requires three arguments",
        ));
    }
    match (plain_vector(&vals[0])?, plain_vector(&vals[1])?) {
        (None, _) | (_, None) => Ok(Value::Null),
        (Some(a), Some(b)) => {
            if a.len() != b.len() {
                return Err(Error::ty("InvalidArgumentValue: vector dimensions differ"));
            }
            Ok(Value::Float(vector_metric(&a, &b, metric)?))
        }
    }
}

pub(crate) fn vector_norm_metric(vals: &[Value], metric: VectorMetric) -> Result<Value> {
    if vals.len() != 1 {
        return Err(Error::syntax(
            "InvalidNumberOfArguments: vector_norm() requires two arguments",
        ));
    }
    if !matches!(metric, VectorMetric::Euclidean | VectorMetric::Manhattan) {
        return Err(Error::syntax("UnknownDistanceMetric"));
    }
    match plain_vector(&vals[0])? {
        None => Ok(Value::Null),
        Some(a) => {
            let zero = vec![0.0; a.len()];
            Ok(Value::Float(vector_metric(&a, &zero, metric)?))
        }
    }
}

pub(crate) fn vector_cosine(vals: &[Value]) -> Result<Value> {
    let (Some(a), Some(b)) = (vals.first(), vals.get(1)) else {
        return Err(Error::ty("vector.similarity.cosine needs two lists"));
    };
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    let (Some(x), Some(y)) = (as_floats(a), as_floats(b)) else {
        return Err(Error::ty("vector.similarity.cosine needs numeric lists"));
    };
    if x.len() != y.len() || x.is_empty() {
        return Err(Error::ty(
            "InvalidArgumentValue: vectors must have same dimensions",
        ));
    }
    if x.iter().chain(&y).any(|f| f.is_nan())
        || x.iter().all(|f| *f == 0.0)
        || y.iter().all(|f| *f == 0.0)
    {
        return Err(Error::ty("InvalidArgumentValue: not a valid vector"));
    }
    let dot: f64 = x.iter().zip(&y).map(|(a, b)| a * b).sum();
    let nx: f64 = x.iter().map(|a| a * a).sum::<f64>().sqrt();
    let ny: f64 = y.iter().map(|a| a * a).sum::<f64>().sqrt();
    if nx == 0.0 || ny == 0.0 {
        return Ok(Value::Float(0.0));
    }
    let raw = (dot / (nx * ny)) as f32 as f64;
    Ok(Value::Float(((1.0 + raw) / 2.0) as f32 as f64))
}

pub(crate) fn vector_euclidean(vals: &[Value]) -> Result<Value> {
    let (Some(a), Some(b)) = (vals.first(), vals.get(1)) else {
        return Err(Error::ty("vector.similarity.euclidean needs two lists"));
    };
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    let (Some(x), Some(y)) = (as_floats(a), as_floats(b)) else {
        return Err(Error::ty("vector.similarity.euclidean needs numeric lists"));
    };
    if x.len() != y.len() {
        return Err(Error::ty(
            "InvalidArgumentValue: vectors must have same dimensions",
        ));
    }
    if x.is_empty() || x.iter().chain(&y).any(|f| f.is_nan()) {
        return Err(Error::ty("InvalidArgumentValue: not a valid vector"));
    }
    let d2: f64 = x.iter().zip(&y).map(|(a, b)| (a - b) * (a - b)).sum();
    Ok(Value::Float((1.0 / (1.0 + d2)) as f32 as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(xs: &[f64]) -> Value {
        Value::List(
            xs.iter()
                .map(|x| Value::Float(*x))
                .collect::<Vec<_>>()
                .into(),
        )
    }

    fn construct(items: Value, dim: i64, coord: &str) -> Result<Value> {
        vector_construct(&[items, Value::Int(dim), Value::String(coord.into())])
    }

    #[test]
    fn integer_coord_coerces_nan_to_zero() {
        let v = construct(list(&[f64::NAN, 2.0]), 2, "INT8").unwrap();
        assert_eq!(
            v,
            Value::Vector {
                coord: VectorCoordKind::Int8,
                values: vec![Value::Int(0), Value::Int(2)].into(),
            }
        );
    }

    #[test]
    fn integer_coord_rejects_out_of_range_and_non_finite() {
        assert!(construct(list(&[128.0]), 1, "INT8").is_err());
        assert!(construct(list(&[f64::INFINITY]), 1, "INT64").is_err());
    }

    #[test]
    fn float32_coord_narrows_but_must_stay_finite() {
        let v = construct(list(&[1.5]), 1, "FLOAT32").unwrap();
        assert_eq!(
            v,
            Value::Vector {
                coord: VectorCoordKind::Float32,
                values: vec![Value::Float(1.5)].into(),
            }
        );
        // 1e300 is a finite f64 but overflows f32.
        assert!(construct(list(&[1e300]), 1, "FLOAT32").is_err());
        assert!(construct(list(&[f64::NAN]), 1, "FLOAT32").is_err());
    }

    #[test]
    fn float64_coord_requires_finite() {
        let v = construct(list(&[1e300]), 1, "FLOAT64").unwrap();
        assert_eq!(
            v,
            Value::Vector {
                coord: VectorCoordKind::Float64,
                values: vec![Value::Float(1e300)].into(),
            }
        );
        assert!(construct(list(&[f64::NAN]), 1, "FLOAT64").is_err());
        assert!(construct(list(&[f64::NEG_INFINITY]), 1, "FLOAT64").is_err());
    }

    #[test]
    fn narrow_float_coords_round_and_overflow_loudly() {
        // FLOAT16: 10 mantissa bits, range +-65504.
        assert_eq!(
            construct(list(&[1.0, 0.1]), 2, "FLOAT16").unwrap(),
            Value::Vector {
                coord: VectorCoordKind::Float16,
                values: vec![Value::Float(1.0), Value::Float(0.0999755859375)].into(),
            }
        );
        assert!(construct(list(&[65520.0]), 1, "FLOAT16").is_err());
        // BFLOAT16: the top 16 bits of a binary32.
        assert_eq!(
            construct(list(&[257.0]), 1, "BFLOAT16").unwrap(),
            Value::Vector {
                coord: VectorCoordKind::BFloat16,
                values: vec![Value::Float(256.0)].into(),
            }
        );
        assert!(construct(list(&[1e39]), 1, "BFLOAT16").is_err());
    }
}
