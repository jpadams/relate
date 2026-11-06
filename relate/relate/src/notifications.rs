//! The query-notification identity and order policy (ADR-0363 decision 6):
//! identical `(kind, description)` pairs collapse to the first occurrence,
//! preserving list order.
//!
//! Both halves of the list import this module downward: the plan-tree
//! projection ([`crate::plan_node::tree_notifications`]) and the
//! statement-scoped merge ([`crate::statement_notifications::merge`]). It is
//! neutral ground, not a re-export through either half, so neither owns the
//! other's policy and the two cannot drift.

use std::collections::HashSet;

use relate_model::wire::Notification;

/// The ADR-0363 decision 6 dedup identity: identical `(kind, description)`
/// entries collapse to the first occurrence, preserving order. The seen set
/// makes the pass linear, so a large EXPLAIN list stays linear. Kinds whose
/// [`relate_model::wire::NotificationKind::dedups_identical`] is `false`
/// are occurrence-scoped and keep every duplicate — Neo4j reports one
/// notification per deprecated call, per clause-shaped deprecation and per
/// repeated pattern.
pub(crate) fn dedup_identical(notifications: Vec<Notification>) -> Vec<Notification> {
    let mut seen = HashSet::with_capacity(notifications.len());
    let mut deduped = Vec::with_capacity(notifications.len());
    for notification in notifications {
        if !notification.kind().dedups_identical() {
            deduped.push(notification);
            continue;
        }
        let identity = (notification.kind(), notification.description().to_string());
        if seen.insert(identity) {
            deduped.push(notification);
        }
    }
    deduped
}
