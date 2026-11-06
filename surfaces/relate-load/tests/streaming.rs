//! Streaming-pull loopback tests (ADR-0158): the shared
//! `relate_test_support::loopback` fixture serves scripted Query API
//! bodies, `load_streaming` pulls them into a fresh store. These are
//! integration tests because the fixture's `loopback_opts` builds the
//! dependency build's `LoadOpts` — a different type from the lib-under-test's
//! inside the crate's own unit tests.

use relate::{Database, Value};
use relate_load::{load_streaming, LoadError, TYPED_ACCEPT_HEADER};
use relate_test_support::loopback::{
    header_values, loopback_opts, spawn_scripted_responses, spawn_scripted_responses_capturing,
    spawn_single_response,
};
use serde_json::{json, Value as Json};

/// One `--query` carrying both shape statements (nodes, then rels that
/// carry their endpoints): one process, one checkpoint, the second
/// statement's rels resolve through the first's nodes (ADR-0158).
#[test]
fn multi_statement_streaming_load() {
    const NODES: &str = r#"{"data":{"fields":["e"],"values":[[{"$type":"Node","_value":{"_element_id":"4:aaa:0","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Ada"}}}}],[{"$type":"Node","_value":{"_element_id":"4:aaa:1","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Bob"}}}}]]},"errors":[]}"#;
    const RELS: &str = r#"{"data":{"fields":["x","e","y"],"values":[[{"$type":"Node","_value":{"_element_id":"4:aaa:0","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Ada"}}}},{"$type":"Relationship","_value":{"_element_id":"5:aaa:0","_start_node_element_id":"4:aaa:0","_end_node_element_id":"4:aaa:1","_type":"KNOWS","_properties":{}}},{"$type":"Node","_value":{"_element_id":"4:aaa:1","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Bob"}}}}]]},"errors":[]}"#;
    let (h, port) = spawn_scripted_responses("200 OK", vec![NODES, RELS]);
    // Memory tier (ADR-0251): the destination store location is incidental
    // to the pull-protocol assertions.
    let db = Database::open(":memory:").unwrap();
    let stats = load_streaming(
        &loopback_opts(
            port,
            "MATCH (n) RETURN n AS e; MATCH (a)-[r]->(b) RETURN a AS x, r AS e, b AS y",
        ),
        &db,
    )
    .unwrap();
    assert_eq!(stats.nodes_new, 2);
    assert_eq!(stats.rels_new, 1);
    let mut conn = db.connect().unwrap();
    let r = conn
        .query("MATCH ()-[r:KNOWS]->() RETURN count(r)")
        .unwrap();
    assert_eq!(r.rows[0][0], Value::Int(1));
    h.join().ok();
}

/// A non-empty destination is a typed refusal (ADR-0158).
#[test]
fn streaming_load_refuses_non_empty_destination() {
    const NODES: &str = r#"{"data":{"fields":["e"],"values":[[{"$type":"Node","_value":{"_element_id":"4:aaa:0","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Ada"}}}}]]},"errors":[]}"#;
    // One body only: the refusal check runs before any HTTP call, so the
    // second pull never connects and no second body is served.
    let (h, port) = spawn_single_response("200 OK", NODES);
    // Memory tier (ADR-0251): the NonEmpty refusal is engine-level — the
    // destination store location is incidental.
    let db = Database::open(":memory:").unwrap();
    let o = loopback_opts(port, "MATCH (n) RETURN n");
    load_streaming(&o, &db).unwrap();
    let err = load_streaming(&o, &db).unwrap_err();
    assert!(matches!(err, LoadError::NonEmpty), "{err:?}");
    h.join().ok();
}

/// `load_streaming` is the one caller that asks for the typed JSON wire
/// format (ADR-0181): every request it sends carries the ladder, and request
/// bodies stay plain JSON parameters (`Content-Type: application/json`).
#[test]
fn streaming_load_requests_typed_json_on_every_statement() {
    const NODES: &str = r#"{"data":{"fields":["e"],"values":[[{"$type":"Node","_value":{"_element_id":"4:aaa:0","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Ada"}}}}]]},"errors":[]}"#;
    let (fixture, port) = spawn_scripted_responses_capturing("200 OK", vec![NODES, NODES]);
    // Memory tier (ADR-0251): the assertions are about request headers; the
    // destination store location is incidental.
    let db = Database::open(":memory:").unwrap();
    load_streaming(
        &loopback_opts(port, "MATCH (n) RETURN n; MATCH (m) RETURN m"),
        &db,
    )
    .unwrap();
    let heads = fixture.join();
    assert_eq!(heads.len(), 2, "one request per statement: {heads:?}");
    for head in &heads {
        let expected_line = format!("accept: {TYPED_ACCEPT_HEADER}");
        assert!(
            head.lines().any(|line| line == expected_line),
            "every pull request must carry the exact line {expected_line:?}: {head:?}"
        );
        assert_eq!(
            header_values(head, "accept"),
            vec![TYPED_ACCEPT_HEADER],
            "exactly one Accept header, the typed ladder: {head:?}"
        );
        assert_eq!(
            header_values(head, "content-type"),
            vec!["application/json"],
            "request bodies stay plain JSON parameters: {head:?}"
        );
    }
}

/// Streaming extraction dedupes elements across batches: a rel row that
/// repeats its endpoint nodes must not duplicate them (ADR-0158).
#[test]
fn streaming_batches_dedupe_repeated_endpoints() {
    let endpoint = json!({
        "$type": "Node",
        "_value": {
            "_element_id": "4:aaa:0",
            "_labels": ["Person"],
            "_properties": {"name": {"$type": "String", "_value": "Ada"}},
        }
    });
    let values: Vec<Json> = (0..10)
        .map(|i| {
            json!([
                endpoint,
                {
                    "$type": "Relationship",
                    "_value": {
                        "_element_id": format!("5:aaa:{i}"),
                        "_start_node_element_id": "4:aaa:0",
                        "_end_node_element_id": "4:aaa:0",
                        "_type": "KNOWS",
                        "_properties": {},
                    }
                },
            ])
        })
        .collect();
    let body = json!({"data": {"fields": ["x", "e"], "values": values}, "errors": []}).to_string();
    let (h, port) = spawn_single_response("200 OK", body);
    // Memory tier (ADR-0251): the assertions are dedupe counts; the
    // destination store location is incidental.
    let db = Database::open(":memory:").unwrap();
    let stats =
        load_streaming(&loopback_opts(port, "MATCH (a)-[r]->(b) RETURN a, r"), &db).unwrap();
    assert_eq!(stats.nodes_new, 1, "the repeated endpoint dedupes");
    assert_eq!(stats.rels_new, 10);
    h.join().ok();
}

/// Pull carries the source schema (ADR-0201): after the data body, the
/// fixture serves `SHOW CONSTRAINTS` / `SHOW INDEXES` responses; the pulled
/// store ends up with the mapped constraint and index, the constraint-backing
/// index row is filtered, and an unmappable constraint kind is counted as
/// skipped rather than failing the pull.
#[test]
fn pull_carries_source_constraints_and_indexes() {
    const NODES: &str = r#"{"data":{"fields":["e"],"values":[[{"$type":"Node","_value":{"_element_id":"4:aaa:0","_labels":["Person"],"_properties":{"email":{"$type":"String","_value":"ada@x.y"},"bio":{"$type":"String","_value":"hi"}}}}]]},"errors":[]}"#;
    const CONSTRAINTS: &str = r#"{"data":{"fields":["name","type","entityType","labelsOrTypes","properties","propertyType"],"values":[["c1","NODE_PROPERTY_UNIQUENESS","NODE",["Person"],["email"],null],["cx","SOME_FUTURE_KIND","NODE",["Person"],["email"],null]]},"errors":[]}"#;
    const INDEXES: &str = r#"{"data":{"fields":["name","type","entityType","labelsOrTypes","properties","owningConstraint","state"],"values":[["c1","RANGE","NODE",["Person"],["email"],"c1","ONLINE"],["i2","TEXT","NODE",["Person"],["bio"],null,"ONLINE"]]},"errors":[]}"#;
    let (h, port) = spawn_scripted_responses("200 OK", vec![NODES, CONSTRAINTS, INDEXES]);
    // Memory tier (ADR-0251): the assertions are schema-mapping counts; the
    // destination store location is incidental.
    let db = Database::open(":memory:").unwrap();
    let stats = load_streaming(&loopback_opts(port, "MATCH (n) RETURN n AS e"), &db).unwrap();
    assert_eq!(stats.nodes_new, 1);
    assert_eq!(stats.schema.constraints_applied, 1, "{:?}", stats.schema);
    assert_eq!(stats.schema.constraints_skipped, 1, "{:?}", stats.schema);
    assert_eq!(stats.schema.indexes_applied, 1, "{:?}", stats.schema);
    assert_eq!(stats.schema.indexes_skipped, 0, "{:?}", stats.schema);
    let mut conn = db.connect().unwrap();
    let r = conn.query("SHOW CONSTRAINTS").unwrap();
    assert_eq!(
        r.rows.len(),
        1,
        "only the mappable constraint: {:?}",
        r.rows
    );
    let name = r
        .header
        .columns
        .iter()
        .position(|c| c == "name")
        .expect("name column");
    assert_eq!(r.rows[0][name], Value::String("c1".into()));
    let r = conn.query("SHOW INDEXES").unwrap();
    let name = r
        .header
        .columns
        .iter()
        .position(|c| c == "name")
        .expect("name column");
    let names: Vec<&Value> = r.rows.iter().map(|row| &row[name]).collect();
    assert!(
        names.iter().any(|v| **v == Value::String("i2".into())),
        "the pulled TEXT index is present: {names:?}"
    );
    h.join().ok();
}
