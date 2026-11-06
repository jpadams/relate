//! Wire values as Cypher literals: the table form, and the text of `cypher`
//! tagged strings in gram. Identifiers and strings are escaped as every
//! Cypher literal is ([`cypher_identifier`], [`quoted_literal`]).

use super::decode::{entries, Node, Rel, Wire};
use crate::{cypher_identifier, quoted_literal};

impl Wire<'_> {
    /// The value as a Cypher literal.
    pub(super) fn cypher(&self) -> String {
        match self {
            Wire::Null => "null".into(),
            Wire::Bool(b) => b.to_string(),
            Wire::Number(n) => n.to_string(),
            Wire::Str(s) => quoted_literal(s, '"'),
            Wire::Node(n) => node(n),
            Wire::Rel(r) => rel(r),
            Wire::Path(p) => {
                let mut out = node(&p.first);
                for (r, n, forward) in &p.steps {
                    out.push_str(&arrow(&rel(r), *forward));
                    out.push_str(&node(n));
                }
                out
            }
            Wire::Point(m) => format!("point({})", map(&entries(m))),
            Wire::Vector { values, dim, ty } => format!(
                "vector([{}], {dim}, {ty})",
                values
                    .iter()
                    .map(Wire::cypher)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Wire::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(Wire::cypher)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Wire::Map(entries) => map(entries),
        }
    }
}

fn map(entries: &[(&str, Wire)]) -> String {
    let parts: Vec<String> = entries
        .iter()
        .map(|(k, v)| format!("{}: {}", cypher_identifier(k), v.cypher()))
        .collect();
    format!("{{{}}}", parts.join(", "))
}

/// ` {props}`, or nothing for none, in `render`'s record form.
pub(super) fn props(entries: &[(&str, Wire)], render: fn(&[(&str, Wire)]) -> String) -> String {
    if entries.is_empty() {
        String::new()
    } else {
        format!(" {}", render(entries))
    }
}

fn node(n: &Node) -> String {
    let labels: String = n
        .labels
        .iter()
        .map(|l| format!(":{}", cypher_identifier(l)))
        .collect();
    format!("({labels}{})", props(&n.props, map))
}

fn rel(r: &Rel) -> String {
    format!("[:{}{}]", cypher_identifier(r.ty), props(&r.props, map))
}

/// A relationship between two nodes of a pattern, pointing the way it does.
pub(super) fn arrow(rel: &str, forward: bool) -> String {
    if forward {
        format!("-{rel}->")
    } else {
        format!("<-{rel}-")
    }
}
