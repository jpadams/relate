//! The route's one guard: the patterns ADR-0186's Decision sends to BFS.

use std::collections::HashSet;

use relate_core::{RelId, Value};
use relate_ir::{
    MatchMode, NodePattern, PathCount, PathMode, PathPattern, PathSelector, PatternElement,
    RelPattern,
};

use crate::eval::expr_vars;
use crate::row::Row;
use crate::shortest::followable;

use super::super::{ElementFilter, GpmFilters};
use crate::pattern_walk::RelFilter;

/// The three elements of a routed pattern and its selector kind.
pub(super) struct Shape<'p> {
    pub start: &'p NodePattern,
    pub rel: &'p RelPattern,
    /// The rel's resolved type filter (LABS-801) — the pattern-level
    /// table's entry for this exact element, found in the same pass that
    /// decoded the shape, never re-derived or re-sniffed.
    pub filter: &'p RelFilter,
    pub end: &'p NodePattern,
    /// The var-length bounds: the rel's own `*m..n` (legacy element shape)
    /// or the quantifier's `{m,n}` (the quantified element every var-length
    /// spelling lowers to, LABS-661) — the inner rel is a plain one-hop.
    pub min: u32,
    pub max: Option<u32>,
    /// All-path selector (every shortest trail) rather than single-path.
    pub all: bool,
}

/// The one guard: the selectors, match and path modes, and pattern shape of
/// ADR-0186's Decision, an empty incoming used-relationship set, and row
/// bindings the search can honor. `None` keeps the pattern on GPM.
///
/// A path-level `WHERE` no longer sends the pattern to GPM: the route
/// applies it *during* the search (see [`super::path_filter`]), so a
/// shortest path that violates the predicate no longer hides the longer
/// shortest path that satisfies it.
pub(super) fn route_shape<'p>(
    path: &'p PathPattern,
    filters: &'p GpmFilters,
    match_mode: MatchMode,
    used: &HashSet<RelId>,
    row: &Row,
) -> Option<Shape<'p>> {
    if match_mode != MatchMode::DifferentRelationships
        || !used.is_empty()
        || !matches!(path.mode, None | Some(PathMode::Walk | PathMode::Trail))
    {
        return None;
    }
    let all = match path.selector.as_ref()? {
        PathSelector::AnyShortest => false,
        PathSelector::AllShortest => true,
        PathSelector::Shortest {
            count: Some(PathCount::Literal(1)),
            group,
        } => *group,
        PathSelector::Shortest { .. } | PathSelector::Any(_) | PathSelector::All => return None,
    };
    let (start, rel, filter, end, min, max) = routed_hop(path, filters)?;
    if min > 1
        || rel.where_clause.is_some()
        || !followable(start, rel, end)
        || rel
            .variable
            .as_ref()
            .is_some_and(|v| v.with_str(|text| row.contains_key(text)))
    {
        return None;
    }
    // GPM checks the end node with the relationship variable already bound;
    // an endpoint map that reads it (or the path variable) stays on GPM.
    // Text render: the late-binding list compares against the String
    // variable sets of the filter walk.
    let late: Vec<String> = rel
        .variable
        .as_ref()
        .map(|v| v.to_string())
        .into_iter()
        .chain(path.path_names().map(|name| name.to_string()))
        .collect();
    let endpoint_ok = |np: &NodePattern| {
        let mut vars = HashSet::new();
        if let Some(props) = &np.properties {
            expr_vars(props, &mut vars);
        }
        np.where_clause.is_none()
            && np
                .variable
                .as_ref()
                .is_none_or(|v| matches!(row.get_name(v), None | Some(Value::Node(_))))
            && !late.iter().any(|v| vars.contains(v.as_str()))
    };
    (endpoint_ok(start) && endpoint_ok(end)).then_some(Shape {
        start,
        rel,
        filter,
        end,
        min,
        max,
        all,
    })
}

/// The routed var-length hop of a three-element pattern: the outer nodes,
/// the traversed rel, and its effective bounds. Every var-length spelling
/// lowers to the quantified element (LABS-661) — `-[r:T*m..n]-` arrives as
/// `[Node, Quantified([anon, one-hop rel, anon]), Node]` with the bounds on
/// the quantifier — so that shape decodes first (the shared
/// `implicit_var_length_rel` decoder); the plain var-length `Relationship`
/// element (hand-built or planner-IR-diagnostic plans) remains the legacy
/// arm.
/// The routed hop's elements, bounds, and resolved filter (LABS-801):
/// the start node, the traversed rel, the rel's filter, the end node,
/// and the effective bounds — one decoded unit.
type RoutedHop<'p> = (
    &'p NodePattern,
    &'p RelPattern,
    &'p RelFilter,
    &'p NodePattern,
    u32,
    Option<u32>,
);

fn routed_hop<'p>(path: &'p PathPattern, filters: &'p GpmFilters) -> Option<RoutedHop<'p>> {
    let elements = path.elements.as_slice();
    if elements.len() != 3 {
        return None;
    }
    let (PatternElement::Node(start), PatternElement::Node(end)) = (&elements[0], &elements[2])
    else {
        return None;
    };
    // The rel element's resolved filter sits at its own index in the
    // pattern-level table (LABS-801) — the shape decode and the filter
    // lookup read one position, so they cannot disagree.
    match (&elements[1], &filters.filters[1]) {
        (PatternElement::Quantified(qpp), ElementFilter::Group(inner)) => {
            let rel = qpp.implicit_var_length_rel()?;
            let ElementFilter::Rel(filter) = &inner[1] else {
                return None;
            };
            // The postfix `->{m,n}` spelling (its origin marker on the inner
            // rel) keeps its conservative GPM path, as it always has: the
            // route's ANY-tie choice differs from the GPM enumeration order
            // the selective-path pins rest on.
            if rel.quantified {
                return None;
            }
            Some((start, rel, filter, end, qpp.min, qpp.max))
        }
        (PatternElement::Relationship(rel), ElementFilter::Rel(filter)) => {
            if !rel.var_length || rel.quantified {
                return None;
            }
            Some((start, rel, filter, end, rel.min_hops, rel.max_hops))
        }
        _ => None,
    }
}
