//! Load a Neo4j / Aura subgraph into [`relate::Database`] (ADR-0080),
//! and push one back out (ADR-0401). The pull entry point is
//! `load_streaming` (ADR-0158); `load_into_database` remains the
//! in-process subgraph upsert API; `push` is the reverse direction.
//!
//! HTTP Query API v2 + env-file credentials (original `--sync` UX);
//! extract + source-scoped upsert + checkpoint (cloud seed path).

mod creds;
mod error;
mod extract;
mod http;
mod materialize;
mod push;
mod response;
mod schema;
mod shape;
mod stream;
mod typed_value;

use serde_json::Value as Json;

pub use creds::{strip_assigned, Credentials};
pub use error::{LoadError, QueryApiError, TypedWireError};
pub use extract::{subgraph_from_v2, Subgraph, SubgraphNode, SubgraphRel};
pub use http::{
    identity_source, query_api_url, run_query, run_query_streaming, run_query_typed_params,
    TYPED_ACCEPT_HEADER, TYPED_CONTENT_TYPE,
};
pub use materialize::{load_into_database, load_subgraph};
pub use push::{
    parse_key_spec, parse_updated_at_spec, push, EntityReport, PolicySpec, PushOpts, PushOutcome,
    PushReport, PushScope, RelMode, UnkeyedAction, UpdatePolicy, DEFAULT_BATCH_SIZE,
};
pub use schema::SchemaStats;
pub use shape::split_statements;
pub use shape::{assert_graph_shape, looks_like_path, resolve_shape_query};
pub use stream::{load_streaming, StreamingLoad};
// The decoder's error type is public because `TypedWireError::error`
// exposes it; `decode_value` itself stays crate-internal (ADR-0181 item 7).
pub use typed_value::DecodeError;

/// Source connection and shape query for [`load_streaming`].
#[derive(Debug, Clone)]
pub struct LoadOpts {
    pub credentials: Credentials,
    pub query: String,
    pub params: serde_json::Map<String, Json>,
    /// Opt-in to sending Basic auth over plaintext `http://` to a non-loopback
    /// Query API host (ADR-0122). Default is `false` (refuse).
    pub allow_plaintext: bool,
}

/// Counts of created vs reused graph elements (ADR-0080), plus the schema
/// replay counts (ADR-0201; only `load_streaming` fills those).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadStats {
    pub nodes_new: usize,
    pub nodes_reused: usize,
    pub rels_new: usize,
    pub rels_reused: usize,
    /// Source schema carried into the pulled store (ADR-0201).
    pub schema: SchemaStats,
}

#[cfg(test)]
mod tests {
    use super::*;
    use relate::Database;
    use relate::Value;

    fn creds(port: u16, database: &str) -> Credentials {
        Credentials {
            uri: format!("http://127.0.0.1:{port}"),
            username: "neo4j".into(),
            password: "pw".into(),
            database: database.into(),
        }
    }

    fn count_nodes(db: &Database) -> i64 {
        let mut conn = db.connect().unwrap();
        match &conn.query("MATCH (n) RETURN count(n)").unwrap().rows[0][0] {
            Value::Int(n) => *n,
            other => panic!("count {other:?}"),
        }
    }

    fn count_rels(db: &Database) -> i64 {
        let mut conn = db.connect().unwrap();
        match &conn.query("MATCH ()-[r]->() RETURN count(r)").unwrap().rows[0][0] {
            Value::Int(n) => *n,
            other => panic!("count {other:?}"),
        }
    }

    const PERSON_A: &str = r#"{"data":{"fields":["n"],"values":[[{"$type":"Node","_value":{"_element_id":"4:aaa:0","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Ada"}}}}]]}}"#;
    const REL_AB: &str = r#"{"data":{"fields":["a","t","b"],"values":[[{"$type":"Node","_value":{"_element_id":"4:aaa:0","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Ada"}}}},{"$type":"Relationship","_value":{"_element_id":"5:aaa:1","_start_node_element_id":"4:aaa:0","_end_node_element_id":"4:aaa:1","_type":"KNOWS","_properties":{"since":{"$type":"Integer","_value":"1"}}}},{"$type":"Node","_value":{"_element_id":"4:aaa:1","_labels":["Person"],"_properties":{"name":{"$type":"String","_value":"Bob"}}}}]]}}"#;

    /// A streaming pull (ADR-0158) checkpoints the loaded data but not the
    /// load-scoped identity map: the manifest has no `pull_identity_path`,
    /// no `pull-identity.json` is written, and reopening the store from that
    /// checkpoint restores the one pulled node.
    #[test]
    fn checkpoint_reopen_keeps_data_not_the_identity_map() {
        let (h, port) = relate_test_support::loopback::spawn_single_response("200 OK", PERSON_A);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        {
            let db = Database::open(&path).unwrap();
            load_streaming(
                &LoadOpts {
                    credentials: creds(port, "neo4j"),
                    query: "MATCH (n) RETURN n".into(),
                    params: serde_json::Map::new(),
                    allow_plaintext: false,
                },
                &db,
            )
            .unwrap();
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(dir.path().join("manifest.json")).unwrap())
                    .unwrap();
            assert!(manifest.get("pull_identity_path").is_none());
            assert!(!dir.path().join("pull-identity.json").exists());
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(count_nodes(&db), 1);
        h.join().ok();
    }

    #[test]
    fn wal_only_reopen_keeps_data_not_the_identity_map() {
        let (h, port) = relate_test_support::loopback::spawn_single_response("200 OK", REL_AB);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        {
            let db = Database::open(&path).unwrap();
            let creds = creds(port, "neo4j");
            let response = http::run_query(
                &creds,
                "MATCH (a)-[t]->(b) RETURN a, t, b",
                &serde_json::Map::new(),
                false,
            )
            .unwrap();
            let g = subgraph_from_v2(&response).unwrap();
            let source = identity_source(&creds).unwrap();
            let mut conn = db.connect().unwrap();
            let (_, stats) = load_subgraph(&mut conn, &g, &source).unwrap();
            assert_eq!(stats.nodes_new, 2);
            assert_eq!(stats.rels_new, 1);
            drop(conn);
            db.leak_without_checkpoint();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(count_nodes(&db), 2);
        assert_eq!(count_rels(&db), 1);
        h.join().ok();
    }
}
