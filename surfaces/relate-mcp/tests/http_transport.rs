//! Streamable-HTTP transport contract, driven through the handler directly
//! (no socket): the RFC 7617 auth story and the JSON-RPC roundtrip
//! (ADR-0232 §1/§4).

use relate::Database;
use relate_auth::AuthConfig;
use relate_mcp::{handle_http, McpServer, McpServerConfig};
use serde_json::{json, Value};

fn server(auth: AuthConfig) -> McpServer {
    McpServer::new(
        Database::open(":memory:").unwrap(),
        McpServerConfig {
            auth,
            ..McpServerConfig::default()
        },
    )
}

// base64("neo4j:s3cret") / base64("neo4j:wrong") — RFC 7617 credentials.
const GOOD: &str = "Basic bmVvNGo6czNjcmV0";
const BAD: &str = "Basic bmVvNGo6d3Jvbmc=";

#[test]
fn http_unauthenticated_call_is_rejected_401() {
    let server = server(AuthConfig::basic("neo4j", "s3cret"));
    for header in [None, Some(BAD)] {
        let response = handle_http(
            &server,
            "POST",
            header,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        );
        assert_eq!(response.status, 401, "header {header:?} must not pass");
        assert!(response.headers.iter().any(|(name, value)| {
            name == "WWW-Authenticate"
                && value == &format!("Basic realm=\"{}\"", relate_auth::REALM)
        }));
    }
}

#[test]
fn http_authenticated_roundtrip_covers_the_tools() {
    let server = server(AuthConfig::basic("neo4j", "s3cret"));

    let init = handle_http(
        &server,
        "POST",
        Some(GOOD),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
    );
    assert_eq!(init.status, 200);
    let init: Value = serde_json::from_str(&init.body).unwrap();
    assert_eq!(init["result"]["protocolVersion"], json!("2025-03-26"));

    let write = handle_http(
        &server,
        "POST",
        Some(GOOD),
        &json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "write_cypher",
                       "arguments": {"statement": "CREATE (:Person {name: 'Ada'})"}},
        })
        .to_string(),
    );
    assert_eq!(write.status, 200);
    let write: Value = serde_json::from_str(&write.body).unwrap();
    assert_eq!(write["result"]["isError"], json!(false));

    let read = handle_http(
        &server,
        "POST",
        Some(GOOD),
        &json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "read_cypher",
                       "arguments": {"statement": "MATCH (p:Person) RETURN p.name AS name"}},
        })
        .to_string(),
    );
    let read: Value = serde_json::from_str(&read.body).unwrap();
    let text = read["result"]["content"][0]["text"].as_str().unwrap();
    let rows: Value = serde_json::from_str(text).unwrap();
    assert_eq!(rows["rows"], json!([{"name": "Ada"}]));
    assert_eq!(rows["columns"], json!(["name"]));
    assert_eq!(rows["columnTypes"], json!(["STRING"]));
}

// The per-behavior pins (202 notifications, 405 non-POST, auth-none 200)
// live in the unit tests in `src/http.rs`; this file keeps what unit tests
// cannot prove — public-API reachability (`handle_http`, `McpServer`,
// `McpServerConfig`, `relate_auth::REALM`) and the full tools
// roundtrip.
