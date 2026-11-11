//! Vector builtins: thin wrappers over the `eval::vector` helpers, plus the
//! `vector_distance`/`vector_norm` special form with its typed
//! distance-metric literal.

use std::collections::BTreeMap;

use relate_core::{Error, Result, Value};
use relate_ir::{Expr, VectorMetric};

use crate::overlay::ExecGraph;

use super::super::vector::{
    vector_construct, vector_cosine, vector_dimension_count, vector_distance_metric,
    vector_euclidean, vector_norm_metric,
};
use super::super::{eval, Row};

/// The `vector_distance`/`vector_norm` special form. Returns `None` when `n`
/// is not one of the two metric functions.
pub(super) fn vector_metric_form(
    graph: &ExecGraph<'_>,
    row: &Row,
    params: &BTreeMap<String, Value>,
    n: &str,
    args: &[Expr],
) -> Option<Result<Value>> {
    let form = MetricForm::for_name(n)?;
    Some(vector_metric(graph, row, params, n, args, form))
}

/// The name-to-behavior mapping for the two metric forms, derived exactly
/// once: the required arity, the wording used in the arity error, and the
/// metric target.
struct MetricForm {
    arity: usize,
    wording: &'static str,
    target: fn(&[Value], VectorMetric) -> Result<Value>,
}

impl MetricForm {
    fn for_name(n: &str) -> Option<Self> {
        Some(match n {
            "vector_distance" => Self {
                arity: 3,
                wording: "three",
                target: vector_distance_metric,
            },
            "vector_norm" => Self {
                arity: 2,
                wording: "two",
                target: vector_norm_metric,
            },
            _ => return None,
        })
    }
}

// CIP-214: `vector_distance`/`vector_norm` take a typed distance-metric
// literal as their final argument (resolved at lowering); any other
// expression in that position is an `UnknownDistanceMetric` error.
fn vector_metric(
    graph: &ExecGraph<'_>,
    row: &Row,
    params: &BTreeMap<String, Value>,
    n: &str,
    args: &[Expr],
    form: MetricForm,
) -> Result<Value> {
    if args.len() != form.arity {
        return Err(Error::syntax(format!(
            "InvalidNumberOfArguments: {n}() requires {} arguments",
            form.wording
        )));
    }
    let Some(Expr::DistanceMetric(metric)) = args.last() else {
        return Err(Error::syntax("UnknownDistanceMetric"));
    };
    let mut vals = Vec::new();
    for a in &args[..args.len() - 1] {
        vals.push(eval(graph, row, a, params)?);
    }
    (form.target)(&vals, *metric)
}

pub(super) fn bi_vector(_graph: &ExecGraph<'_>, _n: &str, vals: Vec<Value>) -> Result<Value> {
    vector_construct(&vals)
}

pub(super) fn bi_vector_cosinesimilarity(
    _graph: &ExecGraph<'_>,
    _n: &str,
    vals: Vec<Value>,
) -> Result<Value> {
    vector_cosine(&vals)
}

pub(super) fn bi_vector_similarity_euclidean(
    _graph: &ExecGraph<'_>,
    _n: &str,
    vals: Vec<Value>,
) -> Result<Value> {
    vector_euclidean(&vals)
}

pub(super) fn bi_vector_dimension_count(
    _graph: &ExecGraph<'_>,
    _n: &str,
    vals: Vec<Value>,
) -> Result<Value> {
    vector_dimension_count(&vals)
}
