//! ADR-0363 decision 12 on the HTTP Query API: the EXPLAIN notifications
//! ride both success envelopes in the Neo4j shape, and the `notifications`
//! key is omitted entirely when the list is empty.

use serde_json::{json, Value as Json};

use super::tests::{begin_tx, db, dispatch, post, seed_person_and_city, tx_post, txs, OPEN};
use crate::v2_envelope::tests::body_json;

/// The ADR-0363 decision 12 notification entry: exactly the Neo4j key set
/// (`code`, `description`, `severity`, `title`, `category` — the `position`
/// slot omitted because no position exists) with the remedy folded into
/// `description` and no engine-native key.
fn assert_cartesian_notification(notifications: &Json) {
    let [notification] = notifications
        .as_array()
        .unwrap_or_else(|| panic!("notifications must be an array: {notifications}"))
        .as_slice()
    else {
        panic!("the disconnected MATCH raises exactly one notification: {notifications}");
    };
    let object = notification
        .as_object()
        .unwrap_or_else(|| panic!("a notification must be an object: {notification}"));
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["category", "code", "description", "severity", "title"],
        "{notification}"
    );
    assert_eq!(
        notification["code"],
        json!("Neo.ClientNotification.Statement.CartesianProduct")
    );
    assert_eq!(notification["title"], json!("Cartesian product"));
    assert_eq!(notification["severity"], json!("INFORMATION"));
    assert_eq!(notification["category"], json!("PERFORMANCE"));
    // The engine Neo4j-shaped serializer appends the effective remedy to
    // the diagnosis instead of emitting a separate key.
    let description = notification["description"].as_str().unwrap();
    assert!(
        description.starts_with("the MATCH patterns share no join anchor (remedy: "),
        "{description}"
    );
    // No position value exists: the slot is omitted, never emitted null.
    assert!(object.get("position").is_none(), "{notification}");
    for native in ["remedy", "gql_status", "operator_id"] {
        assert!(object.get(native).is_none(), "{notification}");
    }
}

#[test]
fn explain_notifications_ride_the_auto_commit_envelope() {
    let db = db();
    seed_person_and_city(&db);
    let reply = dispatch(
        &db,
        &post(
            "neo4j",
            r#"{"statement": "EXPLAIN MATCH (a:Person), (b:City) RETURN a, b"}"#,
        ),
        OPEN,
        &txs(),
    );
    let body = body_json(&reply);
    assert_eq!(reply.status, 200, "{body}");
    assert_cartesian_notification(&body["notifications"]);
}

#[test]
fn explain_notifications_ride_the_transaction_run_envelope() {
    let db = db();
    seed_person_and_city(&db);
    let txs = txs();
    let id = begin_tx(&db, &txs);
    let run = dispatch(
        &db,
        &tx_post(
            &format!("/db/neo4j/query/v2/tx/{id}"),
            r#"{"statement":"EXPLAIN MATCH (a:Person), (b:City) RETURN a, b"}"#,
        ),
        OPEN,
        &txs,
    );
    let run_body = body_json(&run);
    assert_eq!(run.status, 200, "{run_body}");
    assert_cartesian_notification(&run_body["notifications"]);
    // The statement ran in the open transaction, whose block rides the same
    // success envelope.
    assert_eq!(run_body["transaction"]["id"], json!(id));
}

#[test]
fn notifications_key_is_omitted_when_the_list_is_empty_on_both_routes() {
    let db = db();
    let txs = txs();
    let auto = dispatch(
        &db,
        &post("neo4j", r#"{"statement": "RETURN 1 AS n"}"#),
        OPEN,
        &txs,
    );
    let auto_body = body_json(&auto);
    assert_eq!(auto.status, 200, "{auto_body}");
    assert!(auto_body.get("notifications").is_none(), "{auto_body}");

    let id = begin_tx(&db, &txs);
    let run = dispatch(
        &db,
        &tx_post(
            &format!("/db/neo4j/query/v2/tx/{id}"),
            r#"{"statement":"RETURN 1 AS n"}"#,
        ),
        OPEN,
        &txs,
    );
    let run_body = body_json(&run);
    assert_eq!(run.status, 200, "{run_body}");
    assert!(run_body.get("notifications").is_none(), "{run_body}");
}
