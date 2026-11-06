//! Drive the MCP server in-process the way an MCP host would over stdio:
//! seed a store, then walk `initialize` → `tools/list` → `write_cypher` →
//! `get_schema`. Run with:
//!
//! ```bash
//! cargo run -p relate-mcp --example inspect_schema
//! ```

use relate::Database;
use relate_mcp::{McpServer, McpServerConfig};
use serde_json::json;

fn main() {
    let db = Database::open(":memory:").expect("open :memory:");
    let server = McpServer::new(db, McpServerConfig::default());

    let ask = |request: serde_json::Value| {
        let response = server
            .handle_message(&request.to_string())
            .expect("a request gets a response");
        serde_json::from_str::<serde_json::Value>(&response).unwrap()
    };

    let init = ask(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "example", "version": "0"}},
    }));
    println!("serverInfo: {}", init["result"]["serverInfo"]);

    let write = ask(json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "write_cypher",
                   "arguments": {"statement": "CREATE (:Person {name: 'Ada', born: 1815})"}},
    }));
    println!("write_cypher: {}", write["result"]["content"][0]["text"]);

    let schema = ask(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "get_schema", "arguments": {}},
    }));
    let text = schema["result"]["content"][0]["text"].as_str().unwrap();
    let schema: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        schema["Person"]["properties"]["born"]["type"],
        json!("Integer")
    );
    println!("get_schema: {text}");
}
