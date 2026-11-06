//! The mount's handshake (ADR-0442): the initialize result's MCP and host
//! facts, the ordering rule, the notification, and the tools catalog.
//! Helpers are the tests module's — one kit, not a copy.

use super::*;
// ---------------------------------------------------------------------
// The handshake
// ---------------------------------------------------------------------

#[test]
fn the_initialize_result_carries_the_host_facts() {
    let outcome = run_with(opened(), "per-batch", caps());
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    let result = &outcome.responses[0]["result"];
    // MCP's handshake facts: the revision the server answers with, its
    // capabilities, its identity.
    assert_eq!(
        result["protocolVersion"],
        json!(mcp::MCP_PROTOCOL_VERSION),
        "the initialize result names the MCP revision, not an integer protocol"
    );
    assert_eq!(
        result["capabilities"],
        json!({"tools": {"listChanged": false}})
    );
    assert_eq!(result["freshStoreRequired"], json!(false));
    assert_eq!(result["commitPolicy"], json!("per-batch"));
    assert_eq!(result["serverInfo"]["name"], json!("relate"));
    assert_eq!(
        result["caps"]["maxStatementBytes"],
        json!(relate_wire::MAX_STATEMENT_BYTES)
    );
    assert_eq!(
        result["caps"]["maxRowsPerStatement"],
        json!(relate_wire::MAX_ROWS_PER_STATEMENT)
    );
    assert_eq!(
        result["caps"]["maxStatements"],
        json!(relate_wire::MAX_STATEMENTS)
    );
    assert_eq!(
        result["caps"]["maxStreamBytes"],
        json!(relate_wire::MAX_STREAM_BYTES)
    );
    assert_eq!(
        result["caps"]["maxRequestBytes"],
        json!(relate_wire::mcp::request_bytes_cap(
            relate_wire::MAX_STATEMENT_BYTES
        ))
    );
    // The store's capacity rides the same handshake: a store with no cap
    // publishes u64::MAX as its limit, and used == limit - remaining holds
    // there too, so a child needs no second convention.
    let capacity = &result["capacity"];
    assert_eq!(
        capacity["storeLimitBytes"],
        json!(u64::MAX),
        "an uncapped store publishes u64::MAX"
    );
    assert_eq!(
        capacity["storeRemainingBytes"],
        json!(u64::MAX - capacity["storeUsedBytes"].as_u64().unwrap())
    );
}

#[test]
fn the_initialize_facts_report_whole_import_and_the_fresh_store_requirement() {
    let db = Database::open(":memory:").unwrap();
    let mut conn = db.connect_typed(Namespace::Neo4j).unwrap();
    let mut session = Session::new(
        BufReader::new(Cursor::new(format!("{}\n", initialize(1)).into_bytes())),
        Vec::new(),
        Caps::MOUNT,
        &command("whole-import", true),
    );
    let result = session.drive(&db, &mut conn).unwrap();
    assert!(result.is_none(), "no summary was reported");
    let out = session.into_out();
    let response: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(response["result"]["commitPolicy"], json!("whole-import"));
    assert_eq!(response["result"]["freshStoreRequired"], json!(true));
}

#[test]
fn the_first_request_must_be_initialize() {
    let outcome = run_with(
        vec![request(1, mcp::METHOD_TOOLS_LIST, json!({}))],
        "per-batch",
        caps(),
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(
        error.to_string(),
        "the first MCP request must be initialize, got 'tools/list'"
    );
}

/// A foreign client revision is answered with the one this mount serves —
/// MCP's rule for a version the server does not speak — and the session
/// opens on that answer.
#[test]
fn a_foreign_client_version_is_answered_with_the_served_revision() {
    let outcome = run_with(
        vec![request(
            1,
            mcp::METHOD_INITIALIZE,
            json!({"protocolVersion": "1999-01-01", "capabilities": {}}),
        )],
        "per-batch",
        caps(),
    );
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    assert_eq!(
        outcome.responses[0]["result"]["protocolVersion"],
        json!(mcp::MCP_PROTOCOL_VERSION)
    );
}

/// The initialize request must carry MCP's `protocolVersion`; a handshake
/// without one is answered as invalid params and leaves the session
/// uninitialized (the child may retry), so the stream's end is still the
/// missing-initialize refusal.
#[test]
fn an_initialize_without_a_protocol_version_is_refused() {
    let outcome = run_with(
        vec![request(1, mcp::METHOD_INITIALIZE, json!({}))],
        "per-batch",
        caps(),
    );
    let (code, message) = refusal(&outcome.responses[0]);
    assert_eq!(code, mcp::INVALID_PARAMS);
    assert_eq!(
        message,
        "invalid params: initialize requires a string \"protocolVersion\""
    );
    assert_eq!(
        outcome.result.unwrap_err().to_string(),
        "the extension child ended its MCP session without an initialize request"
    );
}

/// MCP's handshake confirmation: the id-less `notifications/initialized`
/// between the handshake and the first request produces no answer and does
/// not disturb the session.
#[test]
fn the_initialized_notification_is_read_and_ignored() {
    let mut input = opened();
    input.push(json!({"jsonrpc": mcp::JSONRPC_VERSION, "method": mcp::METHOD_INITIALIZED}));
    input.push(request(2, mcp::METHOD_TOOLS_LIST, json!({})));
    let outcome = run_with(input, "per-batch", caps());
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    assert_eq!(
        outcome.responses.len(),
        2,
        "the notification is not answered"
    );
    assert_eq!(outcome.responses[1]["id"], json!(2));
    assert!(outcome.responses[1]["result"]["tools"].is_array());
}

/// A message without an id that is not a notification would leave the child
/// waiting for an answer that never comes: the session fails instead of
/// hanging.
#[test]
fn a_request_without_an_id_is_fatal() {
    let mut input = opened();
    input.push(json!({
        "jsonrpc": mcp::JSONRPC_VERSION,
        "method": mcp::METHOD_TOOLS_CALL,
        "params": {"name": mcp::WRITE_CYPHER, "arguments": {"statement": "CREATE (n:Greet)"}},
    }));
    let outcome = run_with(input, "per-batch", caps());
    assert_eq!(
        outcome.result.unwrap_err().to_string(),
        "the MCP request 'tools/call' carries no id; only notifications may omit it"
    );
    assert_eq!(
        node_count(&outcome.db),
        0,
        "the unanswered call never reaches the engine"
    );
}

#[test]
fn an_empty_session_is_the_missing_initialize_refusal() {
    let outcome = run_with(Vec::new(), "per-batch", caps());
    assert_eq!(
        outcome.result.unwrap_err().to_string(),
        "the extension child ended its MCP session without an initialize request"
    );
}

#[test]
fn the_tools_list_answers_the_five_tools() {
    let mut input = opened();
    input.push(request(2, mcp::METHOD_TOOLS_LIST, json!({})));
    let outcome = run_with(input, "per-batch", caps());
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    // The host serves the kit's one catalog: the response is that table, not
    // a second copy the host could drift from.
    assert_eq!(outcome.responses[1]["result"], mcp::extension_tools_list());
}
