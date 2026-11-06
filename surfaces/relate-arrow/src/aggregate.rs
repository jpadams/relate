//! Vectorized aggregation using Arrow compute kernels.
//!
//! Provides SIMD-accelerated aggregation over Arrow arrays for
//! sum, avg, min, max, count operations. These can be used by the engine
//! to accelerate aggregation over large result sets.

use arrow::compute;
use arrow_array::{Array, Float64Array, Int64Array, RecordBatch};
use relate_model::CypherValue;

/// Compute sum of a numeric column in a RecordBatch.
/// Returns None if the column is not numeric or doesn't exist.
pub fn arrow_sum(batch: &RecordBatch, column: &str) -> Option<CypherValue> {
    let col = batch.column_by_name(column)?;

    if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
        Some(CypherValue::Int(compute::sum(arr).unwrap_or(0)))
    } else {
        col.as_any()
            .downcast_ref::<Float64Array>()
            .map(|arr| CypherValue::Float(compute::sum(arr).unwrap_or(0.0)))
    }
}

/// Compute average of a numeric column.
pub fn arrow_avg(batch: &RecordBatch, column: &str) -> Option<CypherValue> {
    let col = batch.column_by_name(column)?;
    let len = col.len() - col.null_count();
    if len == 0 {
        return Some(CypherValue::Null);
    }

    if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
        let sum = compute::sum(arr).unwrap_or(0) as f64;
        Some(CypherValue::Float(sum / len as f64))
    } else if let Some(arr) = col.as_any().downcast_ref::<Float64Array>() {
        let sum = compute::sum(arr).unwrap_or(0.0);
        Some(CypherValue::Float(sum / len as f64))
    } else {
        None
    }
}

/// Compute min of a numeric column.
pub fn arrow_min(batch: &RecordBatch, column: &str) -> Option<CypherValue> {
    let col = batch.column_by_name(column)?;

    if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
        compute::min(arr).map(CypherValue::Int)
    } else if let Some(arr) = col.as_any().downcast_ref::<Float64Array>() {
        compute::min(arr).map(CypherValue::Float)
    } else {
        None
    }
}

/// Compute max of a numeric column.
pub fn arrow_max(batch: &RecordBatch, column: &str) -> Option<CypherValue> {
    let col = batch.column_by_name(column)?;

    if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
        compute::max(arr).map(CypherValue::Int)
    } else if let Some(arr) = col.as_any().downcast_ref::<Float64Array>() {
        compute::max(arr).map(CypherValue::Float)
    } else {
        None
    }
}

/// Count non-null values in a column.
pub fn arrow_count(batch: &RecordBatch, column: &str) -> Option<CypherValue> {
    let col = batch.column_by_name(column)?;
    Some(CypherValue::Int((col.len() - col.null_count()) as i64))
}

/// Compute batch L2 distances between a query vector and a column of Float64 coordinate pairs.
/// The input arrays should contain flattened coordinates (x1, y1, x2, y2, ...).
pub fn batch_l2_distances(
    xs: &Float64Array,
    ys: &Float64Array,
    query_x: f64,
    query_y: f64,
) -> Float64Array {
    let len = xs.len().min(ys.len());
    let mut distances = Vec::with_capacity(len);
    for i in 0..len {
        if xs.is_null(i) || ys.is_null(i) {
            distances.push(f64::NAN);
        } else {
            let dx = xs.value(i) - query_x;
            let dy = ys.value(i) - query_y;
            distances.push((dx * dx + dy * dy).sqrt());
        }
    }
    Float64Array::from(distances)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::StringArray;
    use arrow_schema::{DataType, Field, Schema};
    use std::sync::Arc;

    fn make_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("name", DataType::Utf8, false),
            Field::new("age", DataType::Int64, true),
            Field::new("score", DataType::Float64, true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["Alice", "Bob", "Carol"])),
                Arc::new(Int64Array::from(vec![Some(30), Some(25), Some(35)])),
                Arc::new(Float64Array::from(vec![Some(9.5), Some(8.0), Some(9.0)])),
            ],
        )
        .unwrap()
    }

    #[test]
    fn test_arrow_sum_int() {
        let batch = make_batch();
        assert_eq!(arrow_sum(&batch, "age"), Some(CypherValue::Int(90)));
    }

    #[test]
    fn test_arrow_sum_float() {
        let batch = make_batch();
        match arrow_sum(&batch, "score") {
            Some(CypherValue::Float(f)) => assert!((f - 26.5).abs() < 1e-10),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn test_arrow_avg() {
        let batch = make_batch();
        match arrow_avg(&batch, "age") {
            Some(CypherValue::Float(f)) => assert!((f - 30.0).abs() < 1e-10),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn test_arrow_min_max() {
        let batch = make_batch();
        assert_eq!(arrow_min(&batch, "age"), Some(CypherValue::Int(25)));
        assert_eq!(arrow_max(&batch, "age"), Some(CypherValue::Int(35)));
    }

    #[test]
    fn test_arrow_count() {
        let batch = make_batch();
        assert_eq!(arrow_count(&batch, "age"), Some(CypherValue::Int(3)));
    }

    #[test]
    fn test_arrow_sum_nonexistent_column() {
        let batch = make_batch();
        assert_eq!(arrow_sum(&batch, "nonexistent"), None);
    }

    #[test]
    fn test_arrow_sum_non_numeric() {
        let batch = make_batch();
        assert_eq!(arrow_sum(&batch, "name"), None);
    }

    #[test]
    fn test_batch_l2_distances() {
        let xs = Float64Array::from(vec![1.0, 4.0, 0.0]);
        let ys = Float64Array::from(vec![0.0, 0.0, 3.0]);
        let dists = batch_l2_distances(&xs, &ys, 0.0, 0.0);
        assert!((dists.value(0) - 1.0).abs() < 1e-10);
        assert!((dists.value(1) - 4.0).abs() < 1e-10);
        assert!((dists.value(2) - 3.0).abs() < 1e-10);
    }
}
