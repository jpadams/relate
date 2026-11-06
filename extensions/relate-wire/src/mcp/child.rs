//! The child-side MCP client kit (ADR-0442): the `initialize` handshake,
//! `tools/call` per record the child wants to emit, the capped rows chunker,
//! and the typed [`McpChildError`] refusals.
//!
//! The kit stays pure std + serde_json: a child links it, and a child never
//! touches a store. The host side — reading the child's requests,
//! dispatching tools against its own `Connection`, enforcing the caps,
//! writing the responses — stays in the CLI: it is host execution, not a
//! shared mechanism.
//!
//! [`serve`] is the whole `main` of a first-party child: the stdio client,
//! the handshake, the import closure, and the non-zero exit that reports a
//! failure on stderr — the one channel the host renders.

use std::io::{BufRead, BufReader, Stdout, Write};

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::read_cap::{read_line_capped, write_line, CappedLine};
use crate::{MAX_ROWS_PER_STATEMENT, MAX_STATEMENT_BYTES, MAX_STREAM_BYTES};

use super::protocol::{
    Request, Response, ResponseError, ServerFacts, JSONRPC_VERSION, MCP_PROTOCOL_VERSION,
    METHOD_INITIALIZE, METHOD_INITIALIZED, METHOD_TOOLS_CALL,
};
use super::tools::{ToolCallResult, ToolContent, REPORT_OUTPUT, UPDATE_SCHEMA, WRITE_CYPHER};

/// The framing bytes a chunk's own serialization adds around the rows it
/// carries: serde_json's compact form brackets an array with exactly `[`
/// and `]`, one byte each. The chunker counts this without serializing —
/// the assumption is serde_json's, stated here once instead of implied by
/// two string constants that were never written.
const ARRAY_FRAMING_BYTES: u64 = 2;

/// A refused child-side exchange — typed variants with pinned texts, the
/// host's `ProtocolError` pattern on the child side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpChildError {
    /// The host's request envelope was absent, oversized, or did not
    /// decode as the child's envelope type.
    Envelope {
        /// The refusal, verbatim.
        detail: String,
    },
    /// Writing a request failed: the pipe broke.
    Write {
        /// The I/O refusal, verbatim.
        detail: String,
    },
    /// Reading the host's response failed.
    Read {
        /// The I/O refusal, verbatim.
        detail: String,
    },
    /// The child's whole request stream would exceed the stream cap.
    StreamTooLarge {
        /// The cap the stream would have exceeded, in bytes.
        limit: u64,
    },
    /// One row's serialized bytes alone would fill a call past the statement
    /// cap: no chunking can carry it (ADR-0430 §4's cap, re-anchored per
    /// call).
    RowTooLarge {
        /// The cap the call would have exceeded, in bytes.
        limit: u64,
    },
    /// A response line did not decode as a JSON-RPC 2.0 response.
    MalformedResponse {
        /// The refusal, verbatim.
        detail: String,
    },
    /// A response answered a different id than the request sent.
    IdMismatch {
        /// The id the child sent.
        expected: u64,
        /// The id the response carried.
        got: Value,
    },
    /// The host answered with a JSON-RPC error.
    Rpc {
        /// The error code.
        code: i64,
        /// The error message.
        message: String,
    },
    /// The host's `tools/call` result reported a tool failure.
    Tool {
        /// The tool's failure text.
        text: String,
    },
    /// The host answered with another MCP revision than this child speaks.
    ProtocolMismatch {
        /// The revision the host reported.
        spoke: String,
        /// The revision this child speaks.
        expected: &'static str,
    },
}

impl std::fmt::Display for McpChildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Envelope { detail } => write!(f, "invalid request envelope: {detail}"),
            Self::Write { detail } => write!(f, "failed to write an MCP request: {detail}"),
            Self::Read { detail } => write!(f, "failed to read an MCP response: {detail}"),
            Self::StreamTooLarge { limit } => write!(
                f,
                "the MCP request stream exceeds the {limit}-byte stream cap"
            ),
            Self::RowTooLarge { limit } => {
                write!(f, "a single row exceeds the {limit}-byte statement cap")
            }
            Self::MalformedResponse { detail } => {
                write!(f, "malformed MCP response: {detail}")
            }
            Self::IdMismatch { expected, got } => write!(
                f,
                "the host answered id {got}, but the child sent id {expected}"
            ),
            Self::Rpc { code, message } => {
                write!(f, "the host refused the call ({code}): {message}")
            }
            Self::Tool { text } => f.write_str(text),
            Self::ProtocolMismatch { spoke, expected } => write!(
                f,
                "the host speaks MCP protocol '{spoke}'; this child speaks '{expected}'"
            ),
        }
    }
}

impl std::error::Error for McpChildError {}

impl From<McpChildError> for String {
    /// The `Display` text channel a child's failure report rides: a child's
    /// `run` returns `Result<(), String>`, so `?` on a kit call renders
    /// through this — no per-call-site adapter.
    fn from(error: McpChildError) -> Self {
        error.to_string()
    }
}

/// The outcome of [`McpChild::read_capped_line`]: one line with its
/// terminator stripped, the stream's end, or a line past the cap.
enum LineRead {
    /// One line, its newline stripped.
    Line(Vec<u8>),
    /// The stream ended before the next line.
    Eof,
    /// The line exceeded the cap.
    Exceeded,
}

/// The child's real stdio pair — what a first-party MCP child uses, and what
/// [`serve`] hands the import closure.
pub type StdioChild = McpChild<BufReader<std::io::Stdin>, Stdout>;

/// A first-party child's whole `main` (ADR-0442 §1, ADR-0447 §4): the stdio
/// client, the handshake, the import, and the failure report — the tail
/// every first-party child would otherwise re-write. `main` is one call:
/// `serve(NAME, env!("CARGO_PKG_VERSION"), run)`, where `run` is the child's
/// import over the decoded envelope and the client.
///
/// A child's failure is a non-zero exit with its text on stderr — the one
/// channel the host renders (`the extension child exited with 1: …`) — and
/// that covers both a broken handshake and an import that refuses. Nothing
/// resumable follows a failure: the session is over, so the exit is the
/// report.
pub fn serve<T, F>(name: &str, version: &str, import: F)
where
    T: DeserializeOwned,
    F: FnOnce(&mut StdioChild, T) -> Result<(), String>,
{
    let mut child = McpChild::stdio();
    let outcome = child
        .start::<T>(name, version)
        .map_err(String::from)
        .and_then(|(_facts, envelope)| import(&mut child, envelope));
    if let Some(text) = outcome.err() {
        eprintln!("{text}");
        std::process::exit(1);
    }
}

/// The child's MCP client: one request envelope, then JSON-RPC requests on
/// `out` and the host's responses from `input`.
///
/// [`Self::start`] reads the envelope and performs the `initialize`
/// handshake, returning the host facts and the decoded envelope;
/// [`Self::call`] sends one `tools/call` and returns the tool's result
/// JSON; [`Self::write_rows`] is the chunked `write_cypher`; and
/// [`Self::report_output`] is the `report_output` convenience. The output
/// side counts its bytes against [`MAX_STREAM_BYTES`] and refuses before it
/// would cross the cap, so a runaway child fails loudly instead of being
/// killed by the host.
pub struct McpChild<I: BufRead, O: Write> {
    input: I,
    out: O,
    next_id: u64,
    stream_bytes: u64,
}

impl McpChild<BufReader<std::io::Stdin>, std::io::Stdout> {
    /// The child's real stdio pair — what a first-party MCP child uses.
    pub fn stdio() -> Self {
        Self::new(BufReader::new(std::io::stdin()), std::io::stdout())
    }
}

impl<I: BufRead, O: Write> McpChild<I, O> {
    /// Build the client over an explicit pair (the tests' seam).
    pub fn new(input: I, out: O) -> Self {
        Self {
            input,
            out,
            next_id: 0,
            stream_bytes: 0,
        }
    }

    /// Read the request envelope (one line, bounded) and perform the
    /// `initialize` handshake: send the MCP revision this kit speaks with
    /// its client identity, decode the host's
    /// [`ServerFacts`](super::protocol::ServerFacts), and confirm the
    /// handshake with `notifications/initialized`.
    ///
    /// The envelope is read first because the host writes it before it
    /// starts reading requests; the order is the wire's, not a choice. The
    /// facts are returned here and not retained: the handshake happens once,
    /// so a second copy on the client would be parallel state. A host that
    /// answers with another MCP revision fails the handshake — the mount
    /// speaks one published revision, never a silent downgrade.
    pub fn start<T: DeserializeOwned>(
        &mut self,
        name: &str,
        version: &str,
    ) -> Result<(ServerFacts, T), McpChildError> {
        let envelope = self.read_envelope_line()?;
        let envelope: T =
            serde_json::from_slice(&envelope).map_err(|e| McpChildError::Envelope {
                detail: e.to_string(),
            })?;
        let id = self.next_id();
        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(Value::from(id)),
            method: METHOD_INITIALIZE.to_string(),
            params: Some(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": name, "version": version},
            })),
        };
        self.send(&request)?;
        let result = self.read_result(id)?;
        let facts: ServerFacts =
            serde_json::from_value(result).map_err(|e| McpChildError::MalformedResponse {
                detail: e.to_string(),
            })?;
        if facts.protocol_version != MCP_PROTOCOL_VERSION {
            return Err(McpChildError::ProtocolMismatch {
                spoke: facts.protocol_version,
                expected: MCP_PROTOCOL_VERSION,
            });
        }
        // MCP's handshake ends with the client's confirmation. It is the
        // protocol's shape, not a host requirement — the host reads every
        // notification and answers none.
        self.notify(METHOD_INITIALIZED)?;
        Ok((facts, envelope))
    }

    /// Send one `tools/call` and return the tool's result JSON.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, McpChildError> {
        let id = self.next_id();
        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(Value::from(id)),
            method: METHOD_TOOLS_CALL.to_string(),
            params: Some(json!({"name": tool, "arguments": arguments})),
        };
        self.send(&request)?;
        let result = self.read_result(id)?;
        decode_tool_result(result)
    }

    /// Write `rows` through the `write_cypher` tool, one call per chunk
    /// under the mount's caps (ADR-0430 §4, re-anchored per call): at most
    /// [`MAX_ROWS_PER_STATEMENT`] rows per call, at most
    /// [`MAX_STATEMENT_BYTES`] of serialized rows per call — the chunker
    /// counts the array it is building, so the call it sends is the call
    /// the host admits — and a row that cannot fit a call of its own is the
    /// typed [`McpChildError::RowTooLarge`]. A zero-row phase writes
    /// nothing: its statement's empty batch is a no-op either way.
    pub fn write_rows(
        &mut self,
        statement: &str,
        rows: impl Iterator<Item = Value>,
    ) -> Result<(), McpChildError> {
        // The cap arithmetic runs in the cap's own type (u64, like send's
        // stream accounting): the measured row length is cast once, the
        // bound never.
        let mut chunk: Vec<Value> = Vec::new();
        let mut bytes = ARRAY_FRAMING_BYTES;
        for row in rows {
            let row_bytes = serde_json::to_string(&row)
                .expect("a parameter row serializes")
                .len() as u64;
            // A row that cannot fit an empty chunk alone is refused here —
            // the one bound the chunk arithmetic below cannot recover from:
            // after a flush the chunk is empty and this check is what stops
            // the row from opening a fresh chunk that ships over the cap.
            if ARRAY_FRAMING_BYTES + row_bytes > MAX_STATEMENT_BYTES {
                return Err(McpChildError::RowTooLarge {
                    limit: MAX_STATEMENT_BYTES,
                });
            }
            // The array this row would join: its bytes so far, the comma a
            // second row costs, and the row itself. Over the cap, the chunk
            // ships without the row — which opens the next call instead.
            // The empty-chunk case cannot reach this flush: the bound above
            // already refused the row it would have carried.
            let comma = u64::from(!chunk.is_empty());
            if bytes + comma + row_bytes > MAX_STATEMENT_BYTES {
                self.write_rows_chunk(statement, std::mem::take(&mut chunk))?;
                bytes = ARRAY_FRAMING_BYTES;
            }
            bytes += u64::from(!chunk.is_empty()) + row_bytes;
            chunk.push(row);
            if chunk.len() == MAX_ROWS_PER_STATEMENT {
                self.write_rows_chunk(statement, std::mem::take(&mut chunk))?;
                bytes = ARRAY_FRAMING_BYTES;
            }
        }
        if !chunk.is_empty() {
            self.write_rows_chunk(statement, chunk)?;
        }
        Ok(())
    }

    /// One `write_cypher` call carrying `chunk` as the statement's `$rows`
    /// parameter — the chunk writer [`Self::write_rows`] flushes through.
    fn write_rows_chunk(
        &mut self,
        statement: &str,
        chunk: Vec<Value>,
    ) -> Result<(), McpChildError> {
        self.call(
            WRITE_CYPHER,
            json!({"statement": statement, "parameters": {"rows": chunk}}),
        )
        .map(|_| ())
    }

    /// Execute one DDL statement (an index or a constraint) through the
    /// `update_schema` tool, returning the tool's result JSON — the schema
    /// names the call created.
    pub fn update_schema(&mut self, statement: &str) -> Result<Value, McpChildError> {
        self.call(UPDATE_SCHEMA, json!({"statement": statement}))
    }

    /// Report the run's summary (text plus optional starters) through the
    /// `report_output` tool.
    pub fn report_output(&mut self, text: &str, starters: &[String]) -> Result<(), McpChildError> {
        let arguments = if starters.is_empty() {
            json!({"text": text})
        } else {
            json!({"text": text, "starters": starters})
        };
        self.call(REPORT_OUTPUT, arguments).map(|_| ())
    }

    /// The next correlation id (the handshake took id 1).
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// One capped line off the input, its terminator stripped: the read the
    /// envelope and every response share, so the cap, the newline strip and
    /// the I/O fan-out exist once. Each caller words its own refusal for
    /// the three outcomes.
    fn read_capped_line(&mut self) -> std::io::Result<LineRead> {
        Ok(
            match read_line_capped(&mut self.input, MAX_STATEMENT_BYTES)? {
                CappedLine::Line(mut line) => {
                    if line.last() == Some(&b'\n') {
                        line.pop();
                    }
                    LineRead::Line(line)
                }
                CappedLine::Eof => LineRead::Eof,
                CappedLine::Exceeded => LineRead::Exceeded,
            },
        )
    }

    /// Read the one-line request envelope under the statement cap.
    fn read_envelope_line(&mut self) -> Result<Vec<u8>, McpChildError> {
        match self.read_capped_line() {
            Ok(LineRead::Line(line)) => Ok(line),
            Ok(LineRead::Eof) => Err(McpChildError::Envelope {
                detail: "the host wrote no request envelope".to_string(),
            }),
            Ok(LineRead::Exceeded) => Err(McpChildError::Envelope {
                detail: format!(
                    "the request envelope exceeds the {MAX_STATEMENT_BYTES}-byte statement cap"
                ),
            }),
            Err(e) => Err(McpChildError::Read {
                detail: e.to_string(),
            }),
        }
    }

    /// Send one notification: a request-shaped message without an id, which
    /// the host never answers.
    fn notify(&mut self, method: &str) -> Result<(), McpChildError> {
        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: None,
            method: method.to_string(),
            params: None,
        };
        self.send(&request)
    }

    /// Write one request line, enforcing the stream cap before the write.
    fn send(&mut self, request: &Request) -> Result<(), McpChildError> {
        let line = request.to_line();
        let cost = line.len() as u64 + 1;
        if self.stream_bytes + cost > MAX_STREAM_BYTES {
            return Err(McpChildError::StreamTooLarge {
                limit: MAX_STREAM_BYTES,
            });
        }
        write_line(&mut self.out, &line).map_err(|e| McpChildError::Write {
            detail: e.to_string(),
        })?;
        self.stream_bytes += cost;
        Ok(())
    }

    /// Read one response, correlate it with `id`, and return its result.
    fn read_result(&mut self, id: u64) -> Result<Value, McpChildError> {
        let line = match self.read_capped_line() {
            Ok(LineRead::Line(line)) => line,
            Ok(LineRead::Eof) => {
                return Err(McpChildError::MalformedResponse {
                    detail: "the host closed the response stream".to_string(),
                })
            }
            Ok(LineRead::Exceeded) => {
                return Err(McpChildError::MalformedResponse {
                    detail: format!(
                        "the response exceeds the {MAX_STATEMENT_BYTES}-byte statement cap"
                    ),
                })
            }
            Err(e) => {
                return Err(McpChildError::Read {
                    detail: e.to_string(),
                })
            }
        };
        let response: Response =
            serde_json::from_slice(&line).map_err(|e| McpChildError::MalformedResponse {
                detail: e.to_string(),
            })?;
        // The kit sends numeric ids; any other id (a string, null) is a
        // different correlation, so the comparison is numeric.
        if response.id.as_u64() != Some(id) {
            return Err(McpChildError::IdMismatch {
                expected: id,
                got: response.id,
            });
        }
        match response.into_result() {
            Ok(result) => Ok(result),
            Err(ResponseError::Rpc(error)) => Err(McpChildError::Rpc {
                code: error.code,
                message: error.message,
            }),
            Err(ResponseError::Shape(detail)) => Err(McpChildError::MalformedResponse { detail }),
        }
    }
}

/// Decode the host's `tools/call` result: the one text content parsed as the
/// tool's JSON result, or the tool's failure text. Exactly one content item
/// is the contract — a second item would be silently dropped, so its
/// presence is refused instead.
fn decode_tool_result(result: Value) -> Result<Value, McpChildError> {
    let result: ToolCallResult =
        serde_json::from_value(result).map_err(|e| McpChildError::MalformedResponse {
            detail: e.to_string(),
        })?;
    let [item] = result.content.as_slice() else {
        return Err(McpChildError::MalformedResponse {
            detail: format!(
                "the tools/call result must carry exactly one content item, got {}",
                result.content.len()
            ),
        });
    };
    let ToolContent::Text { text } = item;
    if result.is_error {
        return Err(McpChildError::Tool { text: text.clone() });
    }
    serde_json::from_str(text).map_err(|e| McpChildError::MalformedResponse {
        detail: format!("the tools/call text is not JSON: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::super::protocol::{request_bytes_cap, ServerFacts};
    use super::super::tools::{tool_result, READ_CYPHER, READ_ONLY_REJECTION, WRITE_CYPHER};
    use super::*;
    use crate::{MAX_ROWS_PER_STATEMENT, MAX_STATEMENTS, PER_BATCH};
    use serde_json::json;
    use std::io::Cursor;

    /// The host's initialize result for the fixture manifest facts.
    fn facts_json() -> Value {
        json!({
            "serverInfo": {"name": "relate", "version": "0.13.0"},
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {"tools": {"listChanged": false}},
            "freshStoreRequired": false,
            "commitPolicy": PER_BATCH,
            "caps": {
                "maxStatementBytes": MAX_STATEMENT_BYTES,
                "maxRowsPerStatement": MAX_ROWS_PER_STATEMENT,
                "maxStatements": MAX_STATEMENTS,
                "maxStreamBytes": MAX_STREAM_BYTES,
                "maxRequestBytes": request_bytes_cap(MAX_STATEMENT_BYTES),
            },
            "capacity": {
                "storeLimitBytes": 1 << 20,
                "storeUsedBytes": 1024,
                "storeRemainingBytes": (1 << 20) - 1024,
            },
        })
    }

    /// The scripted host side for the kit's tests: a response line for the
    /// id the child is about to send.
    fn response_line(id: u64, result: Value) -> String {
        Response::success(json!(id), result).to_line() + "\n"
    }

    /// A child whose handshake already succeeded: the host's empty envelope,
    /// the initialize response, then `tail` — the scripted responses the
    /// test's calls consume. Returns the child and the facts the handshake
    /// decoded.
    fn started_child(tail: &str) -> (McpChild<Cursor<Vec<u8>>, Vec<u8>>, ServerFacts) {
        let mut input = String::new();
        input.push_str("{}\n");
        input.push_str(&response_line(1, facts_json()));
        input.push_str(tail);
        let mut child = McpChild::new(Cursor::new(input.into_bytes()), Vec::new());
        let (facts, envelope): (ServerFacts, Value) = child.start("greet", "0.1.0").unwrap();
        assert_eq!(envelope, json!({}));
        (child, facts)
    }

    #[test]
    fn the_server_facts_decode_the_initialize_result() {
        let (_child, facts) = started_child("");
        assert_eq!(facts.protocol_version, MCP_PROTOCOL_VERSION);
        assert!(!facts.capabilities.tools.list_changed);
        assert!(!facts.fresh_store_required);
        assert_eq!(facts.commit_policy.spelling(), PER_BATCH);
        assert_eq!(facts.caps.max_rows_per_statement, MAX_ROWS_PER_STATEMENT);
        assert_eq!(facts.caps.max_statements, MAX_STATEMENTS);
        assert_eq!(facts.caps.max_statement_bytes, MAX_STATEMENT_BYTES);
        assert_eq!(
            facts.caps.max_request_bytes,
            request_bytes_cap(MAX_STATEMENT_BYTES)
        );
        // The child sees the store's capacity in the same handshake: the
        // baseline its own write results count down from.
        assert_eq!(facts.capacity.store_limit_bytes, 1 << 20);
        assert_eq!(facts.capacity.store_used_bytes, 1024);
        assert_eq!(facts.capacity.store_remaining_bytes, (1 << 20) - 1024);
    }

    #[test]
    fn the_child_reads_the_envelope_then_initializes_and_calls() {
        let (mut child, _facts) = started_child(&response_line(
            2,
            tool_result("{\"counts\":{\"nodes_created\":3}}"),
        ));
        let result = child
            .call(
                WRITE_CYPHER,
                json!({"statement": "UNWIND $rows AS row CREATE (n:N) RETURN 1"}),
            )
            .unwrap();
        assert_eq!(result, json!({"counts": {"nodes_created": 3}}));
        let written = String::from_utf8(child.out).unwrap();
        let lines: Vec<Value> = written
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        // MCP's handshake: initialize with the revision and the client
        // identity, then the id-less initialized confirmation, then tools.
        assert_eq!(lines[0]["method"], "initialize");
        assert_eq!(lines[0]["jsonrpc"], JSONRPC_VERSION);
        assert_eq!(lines[0]["id"], json!(1));
        assert_eq!(
            lines[0]["params"]["protocolVersion"],
            json!(MCP_PROTOCOL_VERSION)
        );
        assert_eq!(lines[0]["params"]["clientInfo"]["name"], json!("greet"));
        assert_eq!(lines[1]["method"], METHOD_INITIALIZED);
        assert_eq!(lines[1]["jsonrpc"], JSONRPC_VERSION);
        assert!(
            lines[1].get("id").is_none(),
            "a notification carries no id: {}",
            lines[1]
        );
        assert_eq!(lines[2]["method"], "tools/call");
        assert_eq!(lines[2]["params"]["name"], WRITE_CYPHER);
    }

    #[test]
    fn a_tool_error_surfaces_its_text() {
        let failure = json!({
            "content": [{"type": "text", "text": READ_ONLY_REJECTION}],
            "isError": true,
        });
        let (mut child, _facts) = started_child(&response_line(2, failure));
        let err = child
            .call(READ_CYPHER, json!({"statement": "CREATE (n)"}))
            .unwrap_err();
        assert_eq!(
            err,
            McpChildError::Tool {
                text: READ_ONLY_REJECTION.to_string()
            }
        );
    }

    #[test]
    fn a_second_content_item_is_refused_not_dropped() {
        let two_items = json!({
            "content": [
                {"type": "text", "text": "{}"},
                {"type": "text", "text": "{}"},
            ],
            "isError": false,
        });
        let (mut child, _facts) = started_child(&response_line(2, two_items));
        let err = child.call(REPORT_OUTPUT, json!({"text": "x"})).unwrap_err();
        assert_eq!(
            err.to_string(),
            "malformed MCP response: the tools/call result must carry exactly one content item, got 2"
        );
    }

    #[test]
    fn an_id_mismatch_is_a_pinned_refusal() {
        let (mut child, _facts) = started_child(&response_line(99, json!({})));
        let err = child.call(REPORT_OUTPUT, json!({"text": "x"})).unwrap_err();
        assert_eq!(
            err,
            McpChildError::IdMismatch {
                expected: 2,
                got: json!(99)
            }
        );
        assert_eq!(
            err.to_string(),
            "the host answered id 99, but the child sent id 2"
        );
    }

    #[test]
    fn a_missing_envelope_is_a_pinned_refusal() {
        let mut child = McpChild::new(Cursor::new(Vec::new()), Vec::new());
        let err = child.start::<Value>("greet", "0.1.0").unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid request envelope: the host wrote no request envelope"
        );
    }

    #[test]
    fn a_protocol_mismatch_is_a_pinned_refusal() {
        let mut facts = facts_json();
        facts["protocolVersion"] = json!("1999-01-01");
        let mut input = String::new();
        input.push_str("{}\n");
        input.push_str(&response_line(1, facts));
        let mut child = McpChild::new(Cursor::new(input.into_bytes()), Vec::new());
        let err = child.start::<Value>("greet", "0.1.0").unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("the host speaks MCP protocol '1999-01-01'; this child speaks '{MCP_PROTOCOL_VERSION}'")
        );
    }

    /// The `tools/call` requests a started child wrote, in order.
    fn calls(written: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(written)
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|line: &Value| line["method"] == METHOD_TOOLS_CALL)
            .collect()
    }

    /// A started child whose host answers every call with `result`.
    fn answering_child<T>(answers: usize, result: T) -> McpChild<Cursor<Vec<u8>>, Vec<u8>>
    where
        T: Fn() -> Value,
    {
        let mut tail = String::new();
        for id in 2..(2 + answers as u64) {
            tail.push_str(&response_line(id, result()));
        }
        started_child(&tail).0
    }

    #[test]
    fn write_rows_chunks_at_the_row_cap() {
        // More rows than one call carries: the overflow opens the next
        // call, every row lands, and no call exceeds the row cap.
        let ids = 0..(MAX_ROWS_PER_STATEMENT as i64 + 5);
        let expected_calls = 2;
        let mut child = answering_child(expected_calls, || tool_result("{\"counts\":{}}"));
        child
            .write_rows(
                "UNWIND $rows AS row CREATE (n:N {id: row.id})",
                ids.map(|id| json!({"id": id})),
            )
            .expect("the chunker writes both calls");
        let calls = calls(&child.out);
        assert_eq!(calls.len(), expected_calls, "{calls:?}");
        let rows = |call: &Value| {
            call["params"]["arguments"]["parameters"]["rows"]
                .as_array()
                .expect("the call carries a rows array")
                .len()
        };
        assert_eq!(rows(&calls[0]), MAX_ROWS_PER_STATEMENT);
        assert_eq!(rows(&calls[1]), 5);
        // Every call names the statement and the tool.
        for call in &calls {
            assert_eq!(call["params"]["name"], WRITE_CYPHER);
            assert_eq!(
                call["params"]["arguments"]["statement"],
                json!("UNWIND $rows AS row CREATE (n:N {id: row.id})")
            );
        }
    }

    #[test]
    fn write_rows_joins_one_chunks_rows_under_the_statement_cap() {
        let mut child = answering_child(1, || tool_result("{\"counts\":{}}"));
        child
            .write_rows("X", [json!({"id": 1}), json!({"id": 2})].into_iter())
            .unwrap();
        let calls = calls(&child.out);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0]["params"]["arguments"]["parameters"]["rows"],
            json!([{"id": 1}, {"id": 2}]),
            "one call carries the whole array, never a row per call"
        );
    }

    #[test]
    fn a_row_over_the_cap_behind_a_nonempty_chunk_refuses() {
        // The hole the first-row test cannot see: a SMALL row already sits
        // in the chunk when the oversized one arrives, so the byte-cap
        // branch would flush the small chunk and then — without the
        // single-row bound — open a fresh chunk that ships the oversized
        // row over the cap. The refusal must land BEFORE anything ships.
        let mut child = answering_child(0, || tool_result("{\"counts\":{}}"));
        let huge = "x".repeat(MAX_STATEMENT_BYTES as usize);
        let rows = [json!({ "id": 0 }), json!({ "blob": huge })];
        let err = child.write_rows("X", rows.into_iter()).unwrap_err();
        assert_eq!(
            err,
            McpChildError::RowTooLarge {
                limit: MAX_STATEMENT_BYTES
            },
            "an oversized row behind a non-empty chunk is refused before any flush"
        );
        // Nothing shipped: the small chunk never left the child.
        assert!(calls(&child.out).is_empty());
    }

    #[test]
    fn write_rows_writes_nothing_for_an_empty_phase() {
        // A zero-row phase is a no-op: no call, no transaction.
        let mut child = answering_child(0, || tool_result("{\"counts\":{}}"));
        child.write_rows("X", std::iter::empty()).unwrap();
        assert!(calls(&child.out).is_empty());
    }

    #[test]
    fn a_single_row_over_the_statement_cap_refuses() {
        // A row whose serialized bytes alone fill the statement cap: no
        // chunk can carry it, and the refusal names the cap.
        let mut child = answering_child(0, || tool_result("{\"counts\":{}}"));
        let huge = "x".repeat(MAX_STATEMENT_BYTES as usize);
        let err = child
            .write_rows("X", [json!({"blob": huge})].into_iter())
            .unwrap_err();
        assert_eq!(
            err,
            McpChildError::RowTooLarge {
                limit: MAX_STATEMENT_BYTES
            }
        );
        assert_eq!(
            err.to_string(),
            format!("a single row exceeds the {MAX_STATEMENT_BYTES}-byte statement cap")
        );
        assert!(calls(&child.out).is_empty(), "nothing was sent");
    }

    /// The row the chunker measures is the row the call carries: the
    /// byte arithmetic counts the array, both brackets included, so a
    /// chunk that fits the cap is the chunk the host admits.
    #[test]
    fn the_counted_chunk_is_the_written_chunk() {
        let mut child = answering_child(1, || tool_result("{\"counts\":{}}"));
        let rows = [json!({"id": 1}), json!({"id": 2}), json!({"id": 3})];
        child.write_rows("X", rows.clone().into_iter()).unwrap();
        let calls = calls(&child.out);
        let sent = serde_json::to_string(&calls[0]["params"]["arguments"]["parameters"]["rows"])
            .expect("the array serializes");
        assert_eq!(
            sent,
            serde_json::to_string(&Value::Array(rows.to_vec())).unwrap()
        );
    }

    #[test]
    fn update_schema_sends_the_ddl_statement() {
        let mut child = answering_child(1, || tool_result("{\"created\":[\"file_content\"]}"));
        let result = child
            .update_schema(
                "CREATE FULLTEXT INDEX file_content IF NOT EXISTS FOR (f:File) ON EACH [f.name]",
            )
            .unwrap();
        assert_eq!(result, json!({"created": ["file_content"]}));
        let calls = calls(&child.out);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["params"]["name"], UPDATE_SCHEMA);
    }
}
