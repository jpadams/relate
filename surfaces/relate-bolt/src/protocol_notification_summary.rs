//! The ending-summary notifications policy (ADR-0363 decision 11): the list
//! rides only the SUCCESS that ends the result — the PULL or DISCARD with
//! `has_more == false` — beside the plan pair, never RUN SUCCESS. The entry
//! pairs and their order come from the engine's typed Neo4j-shaped entry
//! function, so the vocabulary, the five keys, their wire order and the
//! remedy folded into `description` are spelled once in `relate-model`.
//! The Bolt 5.2 filter keys shape the list here (ADR-0484 decision 8): the
//! statement's config drops entries below the minimum severity and in
//! disabled categories before the entry is built.

use relate_model::wire::{notification_keys, notification_to_neo4j_entries, Notification};

use crate::fields::NotificationConfig;
use crate::packstream::Pack;

/// One notification's ending-summary map, packed directly from the engine's
/// ordered `(key, value)` entries: `code`, `title`, `description`,
/// `severity`, `category`, with the effective remedy folded into
/// `description`. No JSON object is built or re-read, so the engine's entry
/// order is the wire order.
fn notification_entry(notification: &Notification) -> Pack {
    Pack::Map(
        notification_to_neo4j_entries(notification)
            .into_iter()
            .map(|(key, value)| (Pack::String(key.into()), Pack::String(value)))
            .collect(),
    )
}

/// The ending summary's `notifications` entry (ADR-0363 decision 11,
/// filtered by the statement's ADR-0484 decision 8 config): `None` when
/// the result carries no notifications or the filter dropped them all —
/// the key is omitted — else the key and its packed list. Every entry
/// comes from the engine's typed Neo4j-shaped entries (exactly `code`,
/// `title`, `description`, `severity`, `category`, remedy folded into
/// `description`, no `position`).
pub(crate) fn notifications_summary_pair(
    notifications: &[Notification],
    config: &NotificationConfig,
) -> Option<(Pack, Pack)> {
    let kept: Vec<&Notification> = notifications.iter().filter(|n| config.keeps(n)).collect();
    if kept.is_empty() {
        return None;
    }
    Some((
        Pack::String(notification_keys::NOTIFICATIONS.into()),
        Pack::List(kept.iter().map(|n| notification_entry(n)).collect()),
    ))
}
