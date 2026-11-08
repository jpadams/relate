//! The interned form of a pattern's static label conjunction — the one
//! shared helper under both the walkers' `node_ok_prepared` and the
//! contracted kernels' programs (ADR-0473).

use relate_core::NodeId;
use relate_names::SmallName;

use crate::overlay::ExecGraph;

/// The interned form of a pattern's static label conjunction, resolved
/// once per stage (the kernel stage's `NodePred::prepare`): one interner lookup per label,
/// never per candidate (ADR-0473 — the per-candidate string interning was
/// the measured gap between the first kernel cut and the speed-of-light
/// kernels). A label nothing in the committed store ever carried has no
/// id — no committed node can carry it either, so its check fails without
/// a store read; a write scratch's overlay is the exception (write-path
/// labels stay strings there, interned only at commit), and its nodes
/// answer through the string arm instead.
pub(crate) struct InternedLabels<'a>(Table<'a>);

/// One state at a time: the pattern's names as written (the string arm, no
/// allocation — [`InternedLabels::unresolved`], what `node_ok` passes), or
/// each name paired with its interner id, so a label and its id cannot
/// disagree ([`InternedLabels::resolve`]).
enum Table<'a> {
    /// No label at all: the check is bare visibility.
    Bare,
    Names(&'a [SmallName]),
    Resolved(Vec<(SmallName, Option<u32>)>),
}

impl<'a> InternedLabels<'a> {
    /// The labels as written, checked by name per candidate.
    pub(crate) fn unresolved(labels: &'a [SmallName]) -> Self {
        InternedLabels(if labels.is_empty() {
            Table::Bare
        } else {
            Table::Names(labels)
        })
    }

    /// The labels resolved once against the base interner; owns its names,
    /// so a resolution can outlive the pattern borrow it was built from.
    pub(crate) fn resolve(graph: &ExecGraph<'_>, labels: &[SmallName]) -> InternedLabels<'static> {
        if labels.is_empty() {
            return InternedLabels(Table::Bare);
        }
        InternedLabels(Table::Resolved(
            labels
                .iter()
                .map(|label| {
                    let id = label.with_str(|text| graph.base().interned_label_id(text));
                    (label.clone(), id)
                })
                .collect(),
        ))
    }

    /// The static label conjunction and visibility check — `node_ok`'s two
    /// label arms for an unbound variable: every pattern label present
    /// (`None` for an invisible node fails exactly like an absent label),
    /// and a pattern with no labels at all checking bare visibility.
    pub(crate) fn admits(&self, graph: &ExecGraph<'_>, node: NodeId) -> bool {
        match &self.0 {
            Table::Bare => graph.node_exists(node),
            Table::Names(labels) => labels.iter().all(|label| by_name(graph, node, label)),
            Table::Resolved(labels) => labels.iter().all(|(label, id)| match id {
                Some(id) => matches!(
                    label.with_str(|text| graph.node_has_label_id(node, *id, text)),
                    Some(true)
                ),
                // A label the base interner never recorded can still ride
                // a write scratch: write-path labels stay strings in the
                // overlay, interned only at commit, so the string arm of
                // `node_has_label` serves them. Without a scratch nothing
                // can carry an uninterned label, so the check fails
                // without a store read.
                None => graph.has_write_overlay() && by_name(graph, node, label),
            }),
        }
    }
}

fn by_name(graph: &ExecGraph<'_>, node: NodeId, label: &SmallName) -> bool {
    matches!(
        label.with_str(|text| graph.node_has_label(node, text)),
        Some(true)
    )
}
