//! The MCP mount's behavior tests (ADR-0442): the session driven over
//! scripted pipes against a real in-memory store, so every tool, every
//! cap, and every pinned refusal is asserted without a child process. The
//! real child is the e2e suite's (`tests/e2e_extension_mcp.rs`).

use std::io::{BufReader, Cursor};
use std::time::Duration;

use relate::{Database, Namespace, OpenOptions};
use serde_json::{json, Value};

use relate_wire::mcp::{self, READ_ONLY_REJECTION};

use super::*;
use crate::extensions::manifest::Manifest;
use crate::extensions::write;

/// A command manifest with the given commit policy and
/// fresh-store flag — the session's initialize facts and framing.
fn command(commit: &str, fresh: bool) -> CommandManifest {
    let text = format!(
        r#"{{"kind":"command","name":"mcp-fixture","version":"0.1.0","protocol":"2025-06-18","flags":[],"commit":"{commit}","requires-fresh-store":{fresh},"executable":"run.sh"}}"#
    );
    let manifest: Manifest = serde_json::from_str(&text).unwrap();
    let Manifest::Command(command) = manifest;
    command
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": mcp::JSONRPC_VERSION, "id": id, "method": method, "params": params})
}

/// The MCP `initialize` request: the client's revision and identity.
fn initialize(id: u64) -> Value {
    request(
        id,
        mcp::METHOD_INITIALIZE,
        json!({
            "protocolVersion": mcp::MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "mcp-test", "version": "0.1.0"},
        }),
    )
}

fn call(id: u64, tool: &str, arguments: Value) -> Value {
    request(
        id,
        mcp::METHOD_TOOLS_CALL,
        json!({"name": tool, "arguments": arguments}),
    )
}

/// What one scripted session produced: the drive outcome, the host's
/// response lines, and the store the session ran against.
struct Outcome {
    result: Result<Option<RunSummary>, HostError>,
    responses: Vec<Value>,
    db: Database,
}

/// Run one session over the JSON requests `input`, each as one line.
fn run_with(input: Vec<Value>, commit: &str, caps: Caps) -> Outcome {
    let lines: Vec<String> = input.iter().map(Value::to_string).collect();
    run_raw(lines, commit, caps)
}

/// Run one session over raw request lines (the malformed-line tests need
/// bytes that are not JSON values). The framing bound comes from `caps` by
/// the one derivation ([`Caps::mcp_request_bytes`]) — there is no separate
/// knob to tighten.
fn run_raw(lines: Vec<String>, commit: &str, caps: Caps) -> Outcome {
    run_raw_against(Database::open(":memory:").unwrap(), lines, commit, caps)
}

/// [`run_raw`] against an already-opened store — the tests that need a
/// store property (a capacity cap) build the `Database` first.
fn run_raw_against(db: Database, lines: Vec<String>, commit: &str, caps: Caps) -> Outcome {
    let mut script = String::new();
    for line in lines {
        script.push_str(&line);
        script.push('\n');
    }
    let mut conn = db.connect_typed(Namespace::Neo4j).unwrap();
    let mut session = Session::new(
        BufReader::new(Cursor::new(script.into_bytes())),
        Vec::new(),
        caps,
        &command(commit, false),
    );
    let result = session.drive(&db, &mut conn);
    let out = session.into_out();
    let responses = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    Outcome {
        result,
        responses,
        db,
    }
}

/// The mount's shipped caps.
fn caps() -> Caps {
    Caps::MOUNT
}

/// A session's `tools/call` result text, parsed back to JSON.
fn tool_result(response: &Value) -> Value {
    assert!(
        response.get("error").is_none(),
        "the call must succeed: {response}"
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("a tools/call result carries text content: {response}"));
    serde_json::from_str(text).unwrap()
}

/// A refusal's JSON-RPC code and message.
fn refusal(response: &Value) -> (i64, String) {
    let error = response
        .get("error")
        .unwrap_or_else(|| panic!("the call must be refused: {response}"));
    (
        error["code"].as_i64().unwrap(),
        error["message"].as_str().unwrap().to_string(),
    )
}

/// The store's node count, read through the shared freshness helper.
fn node_count(db: &Database) -> i64 {
    let mut conn = db.connect_typed(Namespace::Neo4j).unwrap();
    write::node_count(&mut conn).unwrap()
}

/// The standard opening every refusal test uses.
fn opened() -> Vec<Value> {
    vec![initialize(1)]
}

// The suite's sections (the extension tree's dir-module pattern):
// `handshake` is the mount's opening, `tools` the five tools with their
// refusals and caps, and `framing` the deadline and the commit policy.
// The helpers above are the one kit they share.
mod framing;
mod handshake;
mod tools;
