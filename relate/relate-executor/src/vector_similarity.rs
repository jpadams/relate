//! CIP-121/CIP-214 vector similarity, matching Neo4j float32 rounding.
//! The stored vector-index option schema (the metric narrowing, the key
//! table, the one parse) lives in [`crate::vector_config`]; this module
//! owns the scoring kernels only.

use relate_core::Value;

use crate::vector_config::StorableMetric;

fn round_f32(x: f64) -> f64 {
    x as f32 as f64
}
/// Eight independent dot-product accumulators. The split into lanes is the
/// only reordering versus a single-accumulator loop; it breaks the
/// serial dependency chain so the compiler can vectorize the loop. The
/// `round_f32` results are contract-identical to the sequential loop
/// (see the equivalence tests below, ADR-0256).
fn dot_unrolled(a: &[f64], b: &[f64]) -> f64 {
    let mut lanes = [0.0f64; 8];
    let mut i = 0;
    while i + 8 <= a.len() {
        lanes[0] += a[i] * b[i];
        lanes[1] += a[i + 1] * b[i + 1];
        lanes[2] += a[i + 2] * b[i + 2];
        lanes[3] += a[i + 3] * b[i + 3];
        lanes[4] += a[i + 4] * b[i + 4];
        lanes[5] += a[i + 5] * b[i + 5];
        lanes[6] += a[i + 6] * b[i + 6];
        lanes[7] += a[i + 7] * b[i + 7];
        i += 8;
    }
    let mut dot = lanes.iter().sum();
    while i < a.len() {
        dot += a[i] * b[i];
        i += 1;
    }
    dot
}

/// The scan score for the stored metric — the dispatch is exhaustive
/// over [`StorableMetric`], so the narrowing, the dispatch, and the
/// kernels it selects cannot drift apart (one authority: the type).
/// Every skip contract lives here: an empty or length-mismatched query,
/// a non-finite coordinate, and the all-zero cosine candidate all skip
/// (None). The zero-query guard is cosine's only (a zero query has no
/// direction; euclidean distance stays defined).
pub(crate) fn similarity_values(
    metric: StorableMetric,
    query: &PreparedQuery<'_>,
    candidate: &[Value],
) -> Option<f64> {
    if query.coords.is_empty() || query.coords.len() != candidate.len() {
        return None;
    }
    match metric {
        StorableMetric::Cosine if query.zero => None,
        StorableMetric::Cosine => cosine_similarity_values(query.coords, query.norm, candidate),
        StorableMetric::Euclidean => euclidean_similarity_values(query.coords, candidate),
    }
}

/// The CIP-121 cosine score for the scan path (ADR-0256), with the
/// float32-rounded `(cos + 1) / 2` chain the pre-ADR-0256 kernel applied.
/// Private: the dispatch in [`similarity_values`] is the module's one
/// entry point, so no caller can bypass the skip contracts it owns
/// (stellar round 3, visibility hygiene).
fn cosine_similarity_values(query: &[f64], query_norm: f64, candidate: &[Value]) -> Option<f64> {
    let cos = round_f32(cosine_raw_values(query, query_norm, candidate)?);
    Some(round_f32((cos + 1.0) / 2.0))
}

/// The CIP-214 euclidean score for the scan path (ADR-0256), with the
/// float32-rounded `1 / (1 + d^2)` chain the pre-ADR-0256 kernel applied.
/// Private, like its cosine twin: [`similarity_values`] is the one entry
/// point.
fn euclidean_similarity_values(query: &[f64], candidate: &[Value]) -> Option<f64> {
    Some(round_f32(
        1.0 / (1.0 + euclidean_squared_raw_values(query, candidate)?),
    ))
}

/// One `Value` list coordinate. `None` when the entry is not a finite
/// number — the candidate-skip half of `vector_operand`'s contract
/// (ADR-0256: scan candidates are never converted to a fresh `Vec<f64>`).
pub(crate) fn value_coord(value: &Value) -> Option<f64> {
    match value {
        Value::Int(x) => Some(*x as f64),
        Value::Float(x) if x.is_finite() => Some(*x),
        _ => None,
    }
}

/// The `Value` coordinate list of a candidate property: a VECTOR or a list.
/// `None` skips the candidate, exactly as `vector_operand` does.
pub(crate) fn value_coords(value: &Value) -> Option<&[Value]> {
    match value {
        Value::Vector { values, .. } | Value::List(values) => Some(values),
        _ => None,
    }
}

/// The query's L2 norm — a per-search constant hoisted out of the scan
/// (ADR-0256). Computed by the same unrolled kernel the per-candidate
/// cosine uses, so the norm is bit-consistent within one search.
fn query_norm(query: &[f64]) -> f64 {
    dot_unrolled(query, query).sqrt()
}

/// The per-search query facts, built once (stellar round 2, std-3): the
/// coordinates, their norm, and the all-zero guard travel together as
/// one type, so no caller can pass a bool that contradicts the slice.
/// (The zero guard is the exact `all(|v| *v == 0.0)` — denormal
/// underflow can zero the norm of a nonzero query, so the norm must not
/// derive the guard.)
pub(crate) struct PreparedQuery<'a> {
    coords: &'a [f64],
    norm: f64,
    zero: bool,
}

impl PreparedQuery<'_> {
    pub(crate) fn prepare(query: &[f64]) -> PreparedQuery<'_> {
        PreparedQuery {
            coords: query,
            norm: query_norm(query),
            zero: query.iter().all(|value| *value == 0.0),
        }
    }
}

/// Raw cosine between an f64 query (norm precomputed) and a `Value`
/// coordinate list, in one pass over the candidate: eight dot lanes and
/// eight candidate-norm lanes.
///
/// `None` when any candidate coordinate is not a finite number, or every
/// candidate coordinate is zero — the skip contracts of `vector_operand`
/// and the zero-vector guard.
fn cosine_raw_values(query: &[f64], query_norm: f64, candidate: &[Value]) -> Option<f64> {
    let mut dot = [0.0f64; 8];
    let mut norm = [0.0f64; 8];
    let mut any_nonzero = false;
    let mut i = 0;
    while i + 8 <= candidate.len() {
        for lane in 0..8 {
            let c = value_coord(&candidate[i + lane])?;
            dot[lane] += query[i + lane] * c;
            norm[lane] += c * c;
            any_nonzero |= c != 0.0;
        }
        i += 8;
    }
    let mut dot_sum: f64 = dot.iter().sum();
    let mut norm_sum: f64 = norm.iter().sum();
    while i < candidate.len() {
        let c = value_coord(&candidate[i])?;
        dot_sum += query[i] * c;
        norm_sum += c * c;
        any_nonzero |= c != 0.0;
        i += 1;
    }
    if !any_nonzero {
        return None;
    }
    Some(dot_sum / (query_norm * norm_sum.sqrt()))
}

/// Raw squared euclidean distance between an f64 query and a `Value`
/// coordinate list. `None` when any candidate coordinate is not a finite
/// number (the `vector_operand` skip contract).
fn euclidean_squared_raw_values(query: &[f64], candidate: &[Value]) -> Option<f64> {
    let mut dist = [0.0f64; 8];
    let mut i = 0;
    while i + 8 <= candidate.len() {
        for lane in 0..8 {
            let c = value_coord(&candidate[i + lane])?;
            let d = query[i + lane] - c;
            dist[lane] += d * d;
        }
        i += 8;
    }
    let mut dist_sum: f64 = dist.iter().sum();
    while i < candidate.len() {
        let c = value_coord(&candidate[i])?;
        let d = query[i] - c;
        dist_sum += d * d;
        i += 1;
    }
    Some(dist_sum)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Sequential single-accumulator reference, the pre-ADR-0256 kernel.
    fn cosine_raw_sequential(a: &[f64], b: &[f64]) -> f64 {
        let mut dot = 0.0;
        let mut na = 0.0;
        let mut nb = 0.0;
        for i in 0..a.len() {
            dot += a[i] * b[i];
            na += a[i] * a[i];
            nb += b[i] * b[i];
        }
        dot / (na.sqrt() * nb.sqrt())
    }

    fn euclidean_squared_sequential(a: &[f64], b: &[f64]) -> f64 {
        a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
    }

    /// Deterministic LCG so the equivalence corpus is fixed across runs and
    /// platforms.
    fn lcg(state: &mut u64) -> f64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*state >> 11) as f64 / (1u64 << 53) as f64) - 0.5
    }

    fn value_list(coords: &[f64]) -> Arc<Vec<Value>> {
        Arc::new(coords.iter().map(|&x| Value::Float(x)).collect())
    }

    /// ADR-0256: the unrolled kernels reorder only the f64 partial sums, so
    /// every float32-rounded score — the CIP-121/CIP-214 user-visible
    /// contract — matches the sequential reference bit-for-bit over a fixed
    /// corpus of random, aligned, tail-carrying, and scale-varied inputs.
    /// The `Value`-list kernels (the scan path) and the `Vec<f64>` kernels
    /// (the reference path) round identically too.
    #[test]
    fn unrolled_kernels_round_identically_to_sequential() {
        let dims = [1, 2, 7, 8, 9, 15, 16, 17, 63, 64, 65, 127, 128, 1536];
        let mut state = 0x5eed_cafe_f00d_bee5u64;
        for &d in &dims {
            for trial in 0..200 {
                let scale = [1e-8, 1e-3, 1.0, 1e3][trial % 4];
                let a: Vec<f64> = (0..d).map(|_| lcg(&mut state) * scale).collect();
                let b: Vec<f64> = (0..d).map(|_| lcg(&mut state) * scale).collect();
                // The scan path: candidate as a Value coordinate list, query
                // norm hoisted per search, compared against the sequential
                // single-accumulator reference with the same rounding chain.
                let b_values = Value::List(value_list(&b));
                let coords = value_coords(&b_values).expect("list coordinates");
                assert_eq!(
                    cosine_similarity_values(&a, query_norm(&a), coords),
                    Some(round_f32(
                        (round_f32(cosine_raw_sequential(&a, &b)) + 1.0) / 2.0
                    )),
                    "cosine d={d} trial={trial}"
                );
                assert_eq!(
                    euclidean_similarity_values(&a, coords),
                    Some(round_f32(
                        1.0 / (1.0 + euclidean_squared_sequential(&a, &b))
                    )),
                    "euclidean d={d} trial={trial}"
                );
            }
        }
    }

    /// The `Value` kernels keep `vector_operand`'s skip contracts: a
    /// non-number or non-finite coordinate, a non-list property, and an
    /// all-zero cosine candidate all skip the candidate (None); euclidean
    /// has no zero guard.
    #[test]
    fn value_kernels_keep_the_skip_contracts() {
        let query = [1.0, 0.0];
        let qn = query_norm(&query);
        let values = |coords: &[Value]| Value::List(Arc::new(coords.to_vec()));

        // (The query/candidate length check lives in `similarity_values`.)
        assert!(
            cosine_raw_values(&query, qn, &[Value::Int(1), Value::Null]).is_none(),
            "non-number coordinate"
        );
        assert!(
            cosine_raw_values(&query, qn, &[Value::Float(f64::NAN), Value::Float(0.0)]).is_none(),
            "non-finite coordinate"
        );
        assert!(
            cosine_raw_values(&query, qn, &[Value::Float(0.0), Value::Float(0.0)]).is_none(),
            "zero-vector candidate"
        );
        assert!(
            euclidean_squared_raw_values(&query, &[Value::Float(0.0), Value::Float(0.0)]).is_some(),
            "euclidean has no zero guard"
        );
        assert!(value_coords(&Value::Null).is_none(), "null property");
        assert!(value_coords(&Value::Int(3)).is_none(), "scalar property");
        assert_eq!(
            value_coords(&values(&[Value::Float(1.0)])).map(|c| c.len()),
            Some(1)
        );
    }
}
