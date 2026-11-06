//! The forced rung's guard vocabulary and emission (ADR-0519's gap
//! classes): the interior-node admission that compiles as a per-state
//! guard (class K), the boundary facts one chain emission carries for the
//! product-form split, the transition predicate a quantified group's own
//! WHERE compiles to (class F), and the builder helpers that emit them —
//! the guard attachment, the split form's open/close allocation, and the
//! predicate compile. Split from [`super`] so the compiler's construction
//! engine (`super`'s builder walk) and the forced-only machinery stay one
//! concern per file; everything but the predicate type (the runner reads
//! it off the compiled NFA's fields) is the parent module's own.

use std::collections::HashSet;

use relate_ir::{Expr, LabelExpr, NodePattern, PatternElement};
use relate_names::SmallName;

use super::super::program::bound_is_row_invariant;
use super::{plain_node, Builder};
use crate::eval::expr_vars;

/// One transition's group predicate (class F): the quantified group's
/// `WHERE` against the variable-to-state map the body's positions
/// define, so the runner can read each referenced variable's last
/// binding off the walk — the arrival node at the transition's landing
/// state, the departure node at its source state, the first-arrival
/// chain's most recent cell at any earlier state.
#[derive(Clone)]
pub(crate) struct TransitionPredicate {
    pub(crate) expr: Expr,
    /// Each referenced variable → the state its body position occupies.
    pub(crate) vars: Vec<(String, u32)>,
}

/// The node element at `i`, or `None` when the position does not hold
/// one (the alternation the compile already proved makes this the
/// defensive arm).
pub(super) fn node_element(elements: &[PatternElement], i: usize) -> Option<&NodePattern> {
    match elements.get(i) {
        Some(PatternElement::Node(node)) => Some(node),
        _ => None,
    }
}

/// An admissible interior node's guard verdict (class K): a plain node
/// carries no admission facts and compiles to no guard, a guarded one
/// compiles its whole pattern as the guard of the state its position
/// occupies. The decline rides the `Option` around it.
// A plan-time transient, the size of the `Option<NodePattern>` it names:
// the pattern moves straight into the builder's guard table, so boxing
// would only add an allocation per guarded position.
#[allow(clippy::large_enum_variant)]
pub(super) enum InteriorGuard {
    /// No admission facts — nothing to check at arrival.
    Plain,
    /// The node pattern itself, checked at every arrival.
    Guard(NodePattern),
}

impl InteriorGuard {
    /// The guard pattern, `None` for the plain node — the boundary
    /// facts' (`ChainEdges`) per-position form.
    pub(super) fn into_pattern(self) -> Option<NodePattern> {
        match self {
            InteriorGuard::Plain => None,
            InteriorGuard::Guard(pattern) => Some(pattern),
        }
    }
}

/// The interior-node admission the forced rung compiles as a per-state
/// guard (class K): the node pattern itself — the runner checks it at
/// every arrival with the walker's own `node_ok`, so the guard is the
/// DFS's own semantics by construction. [`InteriorGuard::Plain`] is the
/// node with no admission facts (no guard); `None` declines: an inline
/// element `WHERE` (per-path semantics the pair-level walk cannot carry),
/// a dynamic label expression, or a property map whose values are not
/// row- and parameter-free (the re-arrival search checks guards without
/// the statement's parameter set, and the walker's own `map_from_expr`
/// would read them).
pub(super) fn interior_guard(node: &NodePattern) -> Option<InteriorGuard> {
    if node.where_clause.is_some()
        || !node.label_expr.as_ref().is_none_or(label_expr_static)
        || matches!(
            &node.label_expr,
            Some(expr) if expr.is_static_conjunction() && node.labels.is_empty()
        )
    {
        return None;
    }
    // Bare means no admission facts at all: no labels, no label
    // expression but the match-everything wildcard (a `Not`/`Or` over an
    // empty label list is a fact the guard must carry), no property map.
    let bare = node.labels.is_empty()
        && node
            .label_expr
            .as_ref()
            .is_none_or(|expr| matches!(expr, LabelExpr::Wildcard))
        && node
            .properties
            .as_ref()
            .is_none_or(|props| matches!(props, Expr::Map(entries) if entries.is_empty()));
    if bare {
        return Some(InteriorGuard::Plain);
    }
    if let Some(Expr::Map(entries)) = &node.properties {
        if !entries
            .iter()
            .all(|(_, value)| bound_is_row_invariant(value) && param_free(value))
        {
            return None;
        }
    } else if node.properties.is_some() {
        // A property map is always the lowered `Map` form; anything else
        // is the walker's own shape, not a guard's.
        return None;
    }
    Some(InteriorGuard::Guard(node.clone()))
}

/// Whether the label expression needs no evaluation context: every
/// variant but `Dynamic` compares labels the walk can read off the node
/// alone. The conjunction/disjunction helpers on `LabelExpr` answer
/// shape questions; this one answers the guard's parameter-freedom
/// question, its own fact.
fn label_expr_static(expr: &LabelExpr) -> bool {
    match expr {
        LabelExpr::Label(_) | LabelExpr::Wildcard => true,
        LabelExpr::Not(inner) => label_expr_static(inner),
        LabelExpr::And(items) | LabelExpr::Or(items) => items.iter().all(label_expr_static),
        LabelExpr::Dynamic { .. } => false,
    }
}

/// Whether the expression mentions a parameter — the guard's
/// property-map values must evaluate without the statement's parameter
/// set. `bound_is_row_invariant` proves the row cannot change a value;
/// this proves the parameter set cannot either.
fn param_free(expr: &Expr) -> bool {
    !expr.any(&mut |expr| matches!(expr, Expr::Parameter(_)))
}

/// The forced rung's boundary facts one chain emission carries — the
/// product construction's inputs. The auto rung and every unrolled or
/// nested chain pass the plain form (no guards, no predicate): the
/// layout those forms emit is the v1 vocabulary's, unchanged.
pub(super) struct ChainEdges<'p> {
    /// The body's first node's guard (every round's start position).
    /// `None` under the auto rung.
    pub(super) first: Option<NodePattern>,
    /// The body's last node's guard (every round's end position).
    pub(super) last: Option<NodePattern>,
    /// The entry state already carries a guard of its own (an interior
    /// context node's) — a round-boundary arrival must not land there.
    pub(super) entry_guarded: bool,
    /// The group WHERE, compiled against the body's own positions once
    /// the states exist.
    pub(super) predicate: Option<&'p Expr>,
    /// The quantifier admits zero rounds — the pass-through epsilon the
    /// split form needs (the plain form's close already sits at the
    /// entry).
    pub(super) zero_rounds: bool,
}

impl ChainEdges<'_> {
    /// The plain form: no guards, no predicate, no split — the auto
    /// rung's layout, which the unrolled and nested forms keep under the
    /// forced rung too (a guarded or predicated body is declined there
    /// before the unroll runs).
    pub(super) fn plain() -> Self {
        Self {
            first: None,
            last: None,
            entry_guarded: false,
            predicate: None,
            zero_rounds: false,
        }
    }

    /// Whether the emission carries round facts — a boundary guard, a
    /// guarded entry, or the group predicate: facts that ride only a
    /// plain one-hop-rel body (an unrolled body's copy stitching has no
    /// guard positions).
    pub(super) fn carries_round_facts(&self) -> bool {
        self.first.is_some()
            || self.last.is_some()
            || self.entry_guarded
            || self.predicate.is_some()
    }

    /// Whether the round's end needs its own `close` state: a round-end
    /// guard, a guarded entry state (a round-boundary arrival must not
    /// land on another position's guard), or the group predicate (its
    /// round-closing transition needs a distinct target). A guarded
    /// post-group node never lands on the round boundary — it takes its
    /// own exit state ([`Builder::split_exit`]).
    pub(super) fn needs_split_close(&self) -> bool {
        self.last.is_some() || self.entry_guarded || self.predicate.is_some()
    }
}

/// Whether a chain body unrolls into copies — a var-length rel or a
/// nested quantified element — the shape whose stitching has no guard
/// positions ([`ChainEdges::carries_round_facts`]' decline).
pub(super) fn body_unrolls(elements: &[PatternElement]) -> bool {
    elements.iter().any(|element| match element {
        PatternElement::Relationship(rel) => rel.var_length,
        PatternElement::Quantified(_) => true,
        PatternElement::Node(_) => false,
    })
}

/// The group WHERE's compile (class F): every referenced name must be one
/// of the body's own node variables (`node_states`, the `(variable,
/// state)` positions the emission walked), and the predicate rides the
/// round-closing hop — the last rel transition the body pushed. A name
/// the body does not bind, or binds twice, declines (the matcher's
/// uniqueness gate owns the first fact in production; this is the
/// hand-built-IR bound). Returns the predicate keyed by its transition.
pub(super) fn compile_transition_predicate(
    predicate: &Expr,
    node_states: Vec<(Option<SmallName>, u32)>,
    last_rel_transition: Option<usize>,
) -> Option<(usize, TransitionPredicate)> {
    let mut referenced = HashSet::new();
    expr_vars(predicate, &mut referenced);
    let mut vars: Vec<(String, u32)> = Vec::new();
    for (name, state) in node_states {
        let Some(name) = name else { continue };
        let name = name.to_string();
        if !referenced.contains(&name) {
            continue;
        }
        // A second position binding a referenced name: its last binding
        // is no one state's arrival — decline.
        if vars.iter().any(|(bound, _)| *bound == name) {
            return None;
        }
        vars.push((name, state));
    }
    if vars.len() != referenced.len() {
        return None;
    }
    Some((
        last_rel_transition?,
        TransitionPredicate {
            expr: predicate.clone(),
            vars,
        },
    ))
}

/// The builder's guard emission — the forced rung's half of the
/// construction, beside the admission rules it applies.
impl Builder<'_> {
    /// Attach `guard` to `state` — a second guard on one state is the
    /// conjunction of every pattern whose position occupies the state.
    pub(super) fn guard(&mut self, state: u32, guard: NodePattern) {
        self.guards.push((state, guard));
    }

    /// Whether `state` carries a guard yet — the split decision's entry
    /// fact.
    pub(super) fn state_guarded(&self, state: u32) -> bool {
        self.guards.iter().any(|(at, _)| *at == state)
    }

    /// The interior-node admission policy, one spelling for the pattern
    /// walk and the chain walk: under the forced rung the node compiles
    /// to the guard of `state`, the state its position occupies (a plain
    /// node to none); under the auto rung it must carry no admission
    /// facts at all (`plain_node` — the v1 NFA has no node-admission
    /// surface). `None` declines.
    pub(super) fn admit_interior(&mut self, node: &NodePattern, state: u32) -> Option<()> {
        if !self.forced {
            return plain_node(node);
        }
        if let InteriorGuard::Guard(guard) = interior_guard(node)? {
            self.guard(state, guard);
        }
        Some(())
    }

    /// The round boundary's states for one chain emission entered at
    /// `entry`: the round's `open` state and the last hop's target.
    /// Under the **product form** (a boundary-carrying chain) `open` is
    /// a fresh state carrying the body's first pattern, entered by
    /// epsilon from `entry`, and the last hop targets a fresh `close`
    /// state carrying the body's last pattern, stitched back to `open`
    /// by the repeat epsilon (and from `entry` by the zero-round
    /// pass-through when the quantifier admits zero rounds). Otherwise
    /// `open` is `entry` itself and the target is the caller's `close`
    /// as given — the plain form's layout, unchanged.
    pub(super) fn split_boundary(
        &mut self,
        entry: u32,
        close: Option<u32>,
        edges: &ChainEdges<'_>,
    ) -> Option<(u32, Option<u32>)> {
        let split_close = edges.needs_split_close();
        let open = if edges.first.is_some() || split_close {
            let open = self.alloc()?;
            if let Some(guard) = &edges.first {
                self.guard(open, guard.clone());
            }
            if open != entry {
                self.epsilons.push((entry, open));
            }
            open
        } else {
            entry
        };
        if !split_close {
            return Some((open, close));
        }
        let close_state = self.alloc()?;
        if let Some(guard) = &edges.last {
            self.guard(close_state, guard.clone());
        }
        // The repeat and the zero-round pass-through, the two ways the
        // walk leaves the round boundary: the next round departs the open
        // state (its guard re-checked on this same node), and a
        // zero-round group skips straight to the close.
        self.epsilons.push((close_state, open));
        if edges.zero_rounds {
            self.epsilons.push((entry, close_state));
        }
        Some((open, Some(close_state)))
    }

    /// The state the post-group node occupies, given the group's `close`
    /// (the state every round ends at). A guarded post-group node gets a
    /// fresh exit state, entered by epsilon from `close`, so its guard is
    /// checked once the group is finished — on `close` itself it would
    /// be checked at every round's end and drop each multi-round walk
    /// whose intermediate round ends fail it. An unguarded one stays on
    /// `close`.
    pub(super) fn split_exit(&mut self, close: u32, exit_guarded: bool) -> Option<u32> {
        if !exit_guarded {
            return Some(close);
        }
        let exit = self.alloc()?;
        self.epsilons.push((close, exit));
        Some(exit)
    }
}
