//! Shared test helpers for the `procedure_rows` test modules (`mod.rs` and
//! the `schema` module): run a built-in procedure against a graph and
//! compare its rows.

use std::collections::BTreeMap;

use relate_core::Value;

use super::{builtin_procedure_rows, executable_signature};
use crate::graph::Graph;
use crate::overlay::ExecGraph;

pub(super) fn string(value: &str) -> Value {
    Value::String(value.into())
}

pub(super) fn rows(graph: &Graph, name: &str) -> Vec<BTreeMap<String, Value>> {
    rows_with_principal(graph, name, None)
}

pub(super) fn rows_with_principal(
    graph: &Graph,
    name: &str,
    principal: Option<&str>,
) -> Vec<BTreeMap<String, Value>> {
    rows_with_args(graph, name, principal, &BTreeMap::new())
}

pub(super) fn rows_with_args(
    graph: &Graph,
    name: &str,
    principal: Option<&str>,
    args: &BTreeMap<String, Value>,
) -> Vec<BTreeMap<String, Value>> {
    let view = ExecGraph::read(graph);
    let signature = executable_signature(name).expect("schema procedure is a registered builtin");
    builtin_procedure_rows(&view, signature, principal, args)
        .expect("schema procedure's rows are produced without error")
}
