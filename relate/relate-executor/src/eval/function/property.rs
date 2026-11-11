//! Property special forms: `property_exists`, whose property-name argument
//! is an identifier literal rather than an evaluated expression.

use std::collections::BTreeMap;

use relate_core::{Error, Result, Value};
use relate_ir::Expr;

use crate::overlay::ExecGraph;

use super::super::{eval, Row};

/// The `property_exists` special form. Returns `None` when `n` is not
/// `property_exists`.
pub(super) fn property_exists_form(
    graph: &ExecGraph<'_>,
    row: &Row,
    params: &BTreeMap<String, Value>,
    n: &str,
    args: &[Expr],
) -> Option<Result<Value>> {
    if n != "property_exists" {
        return None;
    }
    Some(property_exists(graph, row, params, args))
}

// PROPERTY_EXISTS uses an identifier as a property-name literal. It must
// not be evaluated as a row variable (CIP-247).
fn property_exists(
    graph: &ExecGraph<'_>,
    row: &Row,
    params: &BTreeMap<String, Value>,
    args: &[Expr],
) -> Result<Value> {
    if args.len() != 2 {
        return Err(Error::syntax(
            "InvalidNumberOfArguments: property_exists() requires two arguments",
        ));
    }
    let element = eval(graph, row, &args[0], params)?;
    // The property-name argument renders once (this is a dedicated
    // two-argument builtin, not a per-row hot read).
    let property = match &args[1] {
        Expr::Variable(name) => name.to_string(),
        Expr::Null => "null".to_string(),
        Expr::String(name) => name.clone(),
        _ => {
            return Err(Error::invalid_argument_type(
                "property name must be an identifier",
            ))
        }
    };
    Ok(match element {
        Value::Node(node) => Value::Bool(
            graph
                .node(node.id)
                .and_then(|node| node.properties.get(property.as_str()).cloned())
                .is_some_and(|value| !value.is_null()),
        ),
        Value::Relationship(rel) => Value::Bool(
            graph
                .rel(rel.id)
                .and_then(|rel| rel.properties.get(property.as_str()).cloned())
                .is_some_and(|value| !value.is_null()),
        ),
        Value::Null => Value::Null,
        _ => {
            return Err(Error::invalid_argument_type(
                "expected NODE or RELATIONSHIP",
            ))
        }
    })
}
