//! The Query API v2 reply envelope: the wire model (ADR-0232 §3, ADR-0386
//! §3) every reply body serializes through.
//!
//! A reply body is one insertion-ordered list of top-level members
//! ([`ReplyBody`]): the success envelope — `data` first (`fields`, then the
//! materialized rows), then the tail its one constructor builds — and every
//! whole-object reply (the error envelopes, the `GET /` discovery document, a
//! statement-less transaction answer) are the same type through the same
//! [`Serialize`] impl, so a below-threshold reply's bytes are exactly the
//! bytes a streamed one carries. The list, not a `serde_json` map, is the
//! body: member order is the wire contract, so a whole-object reply serializes
//! in the order its members were inserted, never in a map's key order.
//!
//! [`ReplyBody::insert_member`] is the body's one mutator: a present key is
//! replaced in place, a missing key is appended. This module is the wire shape
//! only — `crate::stream` owns the framing that carries it.

use serde::Serialize;
use serde_json::Value as Json;

/// A reply body (ADR-0386 §3/§9): its top-level members in wire order.
pub struct ReplyBody(Vec<(String, Json)>);

impl ReplyBody {
    /// The one constructor: the top-level members in the order they
    /// serialize — `data` first for a success envelope
    /// (`query_api::success_envelope`), constructor order for a whole-object
    /// reply.
    pub fn new<S: Into<String>>(members: impl IntoIterator<Item = (S, Json)>) -> Self {
        Self(
            members
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    /// The member-less body — the 202 rollback answer, `{}` on the wire.
    pub(crate) fn empty() -> Self {
        Self(Vec::new())
    }

    /// Insert one top-level member with JSON-object semantics: a member whose
    /// key is already present is replaced where it stands, a missing one is
    /// appended after the last member. The one mutation path of a reply body,
    /// so no key can appear twice.
    pub(crate) fn insert_member(&mut self, key: &str, value: Json) {
        match self.0.iter_mut().find(|(existing, _)| existing == key) {
            Some((_, existing)) => *existing = value,
            None => self.0.push((key.to_string(), value)),
        }
    }
}

impl Serialize for ReplyBody {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut body = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            body.serialize_entry(key, value)?;
        }
        body.end()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::reply::Reply;

    /// The whole-body view of a reply for assertions: the body serializes
    /// through the seam's one `Serialize` impl. Test-only — it lives beside
    /// the body type it reads, not in a route family's test module.
    pub(crate) fn body_json(reply: &Reply) -> serde_json::Value {
        serde_json::to_value(&reply.body).expect("a reply body serializes to JSON")
    }

    use serde_json::json;

    use super::*;

    /// The one insertion implementation: an existing key keeps its place (the
    /// tx routes re-inserting a tail member), a new key arrives last (their
    /// `transaction` block), and the body never carries one key twice.
    #[test]
    fn insert_member_replaces_in_place_and_appends_new_members() {
        let mut body = ReplyBody::new([
            ("data", json!({"fields": [], "values": []})),
            ("counters", json!({"nodes_created": 0})),
            ("bookmarks", json!([])),
        ]);
        body.insert_member("counters", json!({"nodes_created": 1}));
        body.insert_member("transaction", json!({"id": "t1"}));
        let text = serde_json::to_string(&body).unwrap();
        assert_eq!(
            text,
            concat!(
                r#"{"data":{"fields":[],"values":[]},"#,
                r#""counters":{"nodes_created":1},"bookmarks":[],"#,
                r#""transaction":{"id":"t1"}}"#
            )
        );
        assert_eq!(
            text.matches(r#""counters":"#).count(),
            1,
            "the replaced member appears once: {text}"
        );
    }

    /// A whole-object body serializes in insertion order — the order its
    /// caller inserted the members, never a map's key order (which would sort
    /// these the other way round).
    #[test]
    fn a_whole_object_body_serializes_in_insertion_order() {
        let body = ReplyBody::new([
            ("second", json!(2)),
            ("first", json!(1)),
            ("third", json!(3)),
        ]);
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"second":2,"first":1,"third":3}"#
        );
    }
}
