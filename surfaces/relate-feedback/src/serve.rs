//! The agent serve path (ADR-0588 §4): newline-delimited JSON-RPC 2.0 MCP
//! over a reader/writer pair, exposing exactly one tool, `feedback_submit`.
//!
//! The command `relate feedback serve` wires the process's stdio here; tests
//! drive the same loop over in-memory pipes. Blank lines are ignored, every
//! request line gets exactly one response line, and notifications get none
//! (the same framing as `relate-mcp`'s stdio transport, ADR-0232 §4).
//!
//! The tool handler runs the same [`crate::submit`] pipeline as the shell
//! path: consent and the redaction policy decide before the [`Sender`] seam
//! is reached, so a tool call and a `relate feedback submit` run agree on
//! every outcome.

use std::io::{BufRead, Write};
use std::path::Path;

use chrono::Utc;
use relate_wire::mcp::{
    decode_request, object_member, Response, RpcError, Served, ToolCallResult, ToolContent,
    ToolSpec, INVALID_PARAMS, MCP_PROTOCOL_VERSION, METHOD_INITIALIZE, METHOD_NOT_FOUND,
    METHOD_TOOLS_CALL, METHOD_TOOLS_LIST,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::report::{Category, Context, Input};
use crate::state;
use crate::submit::{self, OutcomeDoc, Request, Sender};

/// The one tool this server lists.
pub const TOOL_NAME: &str = "feedback_submit";

/// The server identity the `initialize` handshake carries.
pub const SERVER_NAME: &str = "relate-feedback";

/// The tool description a client reads — the tool-selection prompt. Fixed
/// text: changing the wording changes how often agents report (ADR-0588 §4).
/// The text limit is [`crate::report::MAX_TEXT_BYTES`] in bytes, the
/// validator's unit.
pub const TOOL_DESCRIPTION: &str = "Call this when relate prevented or complicated your task \
    (a missing capability, an unclear or unhelpful error, behavior that differs from the help, \
    impractical performance) — even when you found a workaround. Describe what you expected and \
    what actually happened in your own words, at most 255 bytes each. Never include queries, \
    parameters, schema names, results, file paths, connection details, credentials, names, or \
    identifiers. Report each distinct problem once, then continue.";

/// The tool's input schema: the four caller-supplied strings, closed to
/// anything else. The category enum comes from [`Category::ALL`]. The text
/// limit is not restated here: JSON Schema's `maxLength` counts characters
/// while the validator counts bytes, so the limit lives in
/// [`TOOL_DESCRIPTION`] in the validator's unit instead.
pub fn input_schema() -> Value {
    let categories: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
    json!({
        "type": "object",
        "properties": {
            "category": {"type": "string", "enum": categories},
            "expected": {"type": "string"},
            "observed": {"type": "string"},
            "command": {"type": "string"},
        },
        "required": ["category", "expected", "observed"],
        "additionalProperties": false,
    })
}

/// The one tool this server lists, in the workspace's one MCP catalog
/// vocabulary: the wire kit renders it, so its annotations and envelope
/// cannot differ from the sibling MCP surfaces'.
const TOOL: ToolSpec = ToolSpec {
    name: TOOL_NAME,
    title: "Submit Feedback",
    description: TOOL_DESCRIPTION,
    input_schema,
    // A report is a write to an external sink, and never destructive; the
    // tool reaches one fixed endpoint, not an open world of entities.
    read_only: false,
    destructive: false,
    served: Served::Feedback,
};

/// The `tools/list` result: exactly the one tool, unprefixed, rendered by
/// the shared catalog renderer.
pub fn tools_list() -> Value {
    relate_wire::mcp::tools_list(std::iter::once(&TOOL), "")
}

/// Everything one served process carries: where state lives, who reports,
/// how the environment is read, and how a validated report is sent.
pub struct Server<'a> {
    /// The config directory holding the consent record and the receipts.
    pub dir: &'a Path,
    /// The sender-owned report fields (version, OS, invoker, commands).
    pub context: Context,
    /// The injected environment lookup the opt-out ladder reads.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// The send seam (the CLI's Mixpanel client, or a test mock).
    pub sender: &'a dyn Sender,
}

/// Serve newline-delimited JSON-RPC on `reader`/`writer` until EOF.
/// Blank lines are ignored; every request line gets exactly one response
/// line; notifications get none.
pub fn serve(
    server: &Server<'_>,
    reader: impl BufRead,
    mut writer: impl Write,
) -> std::io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_message(&line) {
            writer.write_all(response.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }
    Ok(())
}

/// The four strings the tool schema admits. `command` is the one optional
/// field; `error_code`, `agent.model` and `request_id` are not offered to
/// agents (ADR-0588 §4).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolArguments {
    category: String,
    expected: String,
    observed: String,
    command: Option<String>,
}

impl Server<'_> {
    /// Answer one wire line: `None` for a notification, one response line
    /// otherwise.
    pub fn handle_message(&self, body: &str) -> Option<String> {
        let request = match decode_request(body.as_bytes()) {
            Ok(request) => request,
            Err(error) => {
                return Some(
                    Response::error(Value::Null, error.code(), error.to_string()).to_line(),
                );
            }
        };
        // The notification rule frames the whole answer: no id, no answer —
        // whatever the method. Everything else is a request, dispatched below.
        let id = request.id?;
        // The wire kit's one object-or-refuse read, the same one the sibling
        // MCP surfaces use; its plain-text refusal becomes this mount's
        // INVALID_PARAMS error.
        let params = match object_member("params", request.params.as_ref()) {
            Ok(params) => params,
            Err(detail) => {
                return Some(
                    Response::error(id, INVALID_PARAMS, format!("Invalid params: {detail}"))
                        .to_line(),
                );
            }
        };
        Some(match self.dispatch(&request.method, params) {
            Ok(result) => Response::success(id, result).to_line(),
            Err(error) => Response::error(id, error.code, error.message).to_line(),
        })
    }

    fn dispatch(
        &self,
        method: &str,
        params: Option<&Map<String, Value>>,
    ) -> Result<Value, RpcError> {
        match method {
            METHOD_INITIALIZE => Ok(self.initialize_result(params)),
            "ping" => Ok(json!({})),
            METHOD_TOOLS_LIST => Ok(tools_list()),
            METHOD_TOOLS_CALL => self.call_tool(params),
            other => Err(RpcError {
                code: METHOD_NOT_FOUND,
                message: format!("Method not found: {other}"),
            }),
        }
    }

    fn initialize_result(&self, params: Option<&Map<String, Value>>) -> Value {
        let requested = params
            .and_then(|params| params.get("protocolVersion"))
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty());
        json!({
            // Echo the client's version when it sent one: the tool surface
            // is identical across the protocol versions in the field.
            "protocolVersion": requested.unwrap_or(MCP_PROTOCOL_VERSION),
            "capabilities": {"tools": {}},
            "serverInfo": {
                "name": SERVER_NAME,
                "version": self.context.cli_version,
            },
        })
    }

    fn call_tool(&self, params: Option<&Map<String, Value>>) -> Result<Value, RpcError> {
        let Some(name) = params
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str)
        else {
            return Err(RpcError {
                code: INVALID_PARAMS,
                message: "Invalid params: tools/call needs a string \"name\"".to_string(),
            });
        };
        let arguments = object_member("arguments", params.and_then(|p| p.get("arguments")))
            .map_err(|detail| RpcError {
                code: INVALID_PARAMS,
                message: format!("Invalid params: {detail}"),
            })?;
        // Malformed arguments and an unknown tool are isError text results,
        // never a CLI error: the client sees the refusal in the tool result.
        let reply = self.run_tool(name, arguments);
        Ok(serde_json::to_value(ToolCallResult {
            content: vec![ToolContent::Text { text: reply.text }],
            is_error: reply.is_error,
        })
        .expect("a tools/call result serializes"))
    }

    /// Run `feedback_submit` over the shared submit pipeline.
    fn run_tool(&self, name: &str, arguments: Option<&Map<String, Value>>) -> ToolReply {
        if name != TOOL_NAME {
            return ToolReply::refused(format!("unknown tool: {name}"));
        }
        let arguments = Value::Object(arguments.cloned().unwrap_or_default());
        let args: ToolArguments = match serde_json::from_value(arguments) {
            Ok(args) => args,
            Err(error) => {
                return ToolReply::refused(format!("invalid {TOOL_NAME} arguments: {error}"));
            }
        };
        let input = Input {
            category: args.category,
            expected: args.expected,
            observed: args.observed,
            command: args.command,
            ..Input::default()
        };
        let doc = submit::submit(
            &Request {
                dir: self.dir,
                input: &input,
                context: &self.context,
                env: self.env,
                // Read per call, beside the consent read `submit` performs:
                // a `telemetry off` or DO_NOT_TRACK after the server started
                // is honoured by the running session.
                config_telemetry: state::load_config_telemetry(self.dir),
                now: Utc::now(),
            },
            self.sender,
        );
        ToolReply::ran(doc)
    }
}

/// One `tools/call` answer: the text the client reads plus its `isError`
/// flag — a refused call and a pipeline outcome differ only in that flag.
struct ToolReply {
    text: String,
    is_error: bool,
}

impl ToolReply {
    /// A malformed call or an unknown tool: always a tool error.
    fn refused(text: String) -> Self {
        Self {
            text,
            is_error: true,
        }
    }

    /// A submit outcome; `isError` follows the outcome's one success
    /// predicate, so the tool and the CLI exit code cannot disagree.
    fn ran(doc: OutcomeDoc) -> Self {
        Self {
            is_error: !doc.outcome().is_success(),
            text: serde_json::to_string_pretty(&doc).expect("an outcome document serializes"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorization::FeedbackConsent;
    use crate::outcome::Reason;
    use crate::report::MAX_TEXT_BYTES;
    use crate::test_support::{context, env_of, MockSender};

    fn serve_lines(
        dir: &Path,
        env: &dyn Fn(&str) -> Option<String>,
        sender: &MockSender,
        lines: &str,
    ) -> Vec<Value> {
        let context = context();
        let server = Server {
            dir,
            context,
            env,
            sender,
        };
        let mut output = Vec::new();
        serve(&server, lines.as_bytes(), &mut output).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn call_line(id: u64) -> String {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"feedback_submit\",\"arguments\":{{\"category\":\"unclear_error\",\"expected\":\"the info command to print the version\",\"observed\":\"it printed help instead\"}}}}}}"
        )
    }

    fn tool_result_content(response: &Value) -> (Value, bool) {
        let result = &response["result"];
        let is_error = result["isError"].as_bool().unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        (serde_json::from_str(text).unwrap(), is_error)
    }

    #[test]
    fn the_tool_list_has_exactly_the_one_feedback_tool_with_the_contract_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let sender = MockSender::new(Reason::Sent);
        let responses = serve_lines(
            tmp.path(),
            &env_of(&[]),
            &sender,
            concat!(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n",
                "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
            ),
        );
        assert_eq!(responses[0]["result"]["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(responses[0]["result"]["serverInfo"]["version"], "1.0.2");
        let tools = responses[1]["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1, "one tool");
        assert_eq!(tools[0]["name"], TOOL_NAME);
        // The entry is the shared renderer's: its annotations are the
        // tool's declared hints.
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], false);
        assert_eq!(tools[0]["annotations"]["destructiveHint"], false);
        let schema = &tools[0]["inputSchema"];
        assert_eq!(
            schema["properties"]["category"]["enum"],
            json!([
                "unclear_error",
                "unexpected_behavior",
                "missing_capability",
                "other_friction"
            ])
        );
        // No `maxLength`: it would count characters while the validator
        // counts bytes, advertising a multi-byte report it then rejects.
        for field in ["expected", "observed"] {
            assert!(
                schema["properties"][field].get("maxLength").is_none(),
                "the schema states no character limit: {field}"
            );
        }
        assert!(
            TOOL_DESCRIPTION.contains(&format!("{MAX_TEXT_BYTES} bytes")),
            "the description states the validator's limit and unit: {TOOL_DESCRIPTION}"
        );
        assert_eq!(schema["properties"]["command"]["type"], "string");
        assert_eq!(
            schema["required"],
            json!(["category", "expected", "observed"])
        );
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn a_call_without_consent_reports_the_submit_paths_skip() {
        let tmp = tempfile::tempdir().unwrap();
        let sender = MockSender::new(Reason::Sent);
        let responses = serve_lines(tmp.path(), &env_of(&[]), &sender, &call_line(3));
        let (outcome, is_error) = tool_result_content(&responses[0]);
        // The same document the submit pipeline produces without consent.
        let input = Input {
            category: "unclear_error".to_string(),
            expected: "the info command to print the version".to_string(),
            observed: "it printed help instead".to_string(),
            ..Input::default()
        };
        let context = context();
        let direct = submit::submit(
            &Request {
                dir: tmp.path(),
                input: &input,
                context: &context,
                env: &env_of(&[]),
                config_telemetry: state::ConfigTelemetry::Missing,
                now: Utc::now(),
            },
            &sender,
        );
        assert_eq!(outcome["outcome"], "skipped");
        assert_eq!(outcome["reason"], "not_enabled");
        assert_eq!(
            outcome["outcome"],
            serde_json::to_value(direct.outcome()).unwrap()
        );
        assert_eq!(
            outcome["reason"],
            serde_json::to_value(direct.reason).unwrap()
        );
        assert!(!is_error, "a skip is not a tool error");
        assert_eq!(sender.calls(), 0, "nothing is sent");
        assert!(outcome["receipt_path"].is_null());
    }

    #[test]
    fn a_call_with_consent_gets_the_sent_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        let mut record = FeedbackConsent::default();
        record.authorize(Utc::now(), "http://127.0.0.1:9/track".to_string());
        record.save(tmp.path()).unwrap();
        let sender = MockSender::new(Reason::Sent);
        let responses = serve_lines(tmp.path(), &env_of(&[]), &sender, &call_line(4));
        let (outcome, is_error) = tool_result_content(&responses[0]);
        assert_eq!(outcome["outcome"], "sent");
        assert_eq!(outcome["reason"], "sent");
        assert_eq!(sender.calls(), 1);
        assert!(!is_error);
        assert!(outcome["report_id"].is_string());
        let receipt = outcome["receipt_path"].as_str().unwrap();
        assert_eq!(
            receipt,
            crate::receipt::receipts_path(tmp.path()).to_str().unwrap()
        );
        assert!(std::path::Path::new(receipt).exists());
        assert_eq!(
            outcome["payload"]["expected"],
            "the info command to print the version"
        );
    }

    /// A `telemetry off` recorded while the server runs is honoured by the
    /// next tool call: the config layer is read per call, beside the consent
    /// record, so a session that outlives the opt-out stops sending.
    #[test]
    fn a_mid_session_telemetry_opt_out_is_honoured() {
        let tmp = tempfile::tempdir().unwrap();
        let mut record = FeedbackConsent::default();
        record.authorize(Utc::now(), "http://127.0.0.1:9/track".to_string());
        record.save(tmp.path()).unwrap();
        let sender = MockSender::new(Reason::Sent);
        let env = env_of(&[]);
        let server = Server {
            dir: tmp.path(),
            context: context(),
            env: &env,
            sender: &sender,
        };
        let first = server.handle_message(&call_line(20)).unwrap();
        let first: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(tool_result_content(&first).0["outcome"], "sent");

        crate::state::save_config_telemetry(tmp.path(), false).unwrap();

        let second = server.handle_message(&call_line(21)).unwrap();
        let second: Value = serde_json::from_str(&second).unwrap();
        let (outcome, is_error) = tool_result_content(&second);
        assert_eq!(outcome["reason"], "telemetry_off");
        assert!(!is_error, "a skip is not a tool error");
        assert_eq!(sender.calls(), 1, "the opted-out call is not sent");
    }

    #[test]
    fn malformed_arguments_and_unknown_tools_are_is_error_results() {
        let tmp = tempfile::tempdir().unwrap();
        let sender = MockSender::new(Reason::Sent);
        let responses = serve_lines(
            tmp.path(),
            &env_of(&[]),
            &sender,
            concat!(
                "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"feedback_submit\",\"arguments\":{\"expected\":\"x\"}}}\n",
                "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"other\"}}\n",
            ),
        );
        for response in &responses {
            assert!(
                response["result"]["isError"].as_bool().unwrap(),
                "a refusal is a tool error"
            );
            assert!(response["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|text| text.starts_with("invalid feedback_submit arguments")
                    || text.starts_with("unknown tool")));
        }
        assert_eq!(sender.calls(), 0);
    }

    /// A `params` or `arguments` that is not an object is the wire kit's
    /// refusal, mapped to this mount's INVALID_PARAMS error — the same answer
    /// the sibling MCP surfaces give, never a silent coercion to null.
    #[test]
    fn a_non_object_params_or_arguments_is_an_invalid_params_error() {
        let tmp = tempfile::tempdir().unwrap();
        let sender = MockSender::new(Reason::Sent);
        let responses = serve_lines(
            tmp.path(),
            &env_of(&[]),
            &sender,
            concat!(
                "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/list\",\"params\":4}\n",
                "{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"tools/call\",\"params\":{\"name\":\"feedback_submit\",\"arguments\":4}}\n",
            ),
        );
        for (response, member) in responses.iter().zip(["params", "arguments"]) {
            assert_eq!(response["error"]["code"], INVALID_PARAMS, "{response}");
            assert!(
                response["error"]["message"].as_str().is_some_and(
                    |message| message.contains(&format!("\"{member}\" must be an object"))
                ),
                "{member}: {response}"
            );
        }
        assert_eq!(sender.calls(), 0);
    }

    #[test]
    fn notifications_and_blank_lines_get_no_response() {
        let tmp = tempfile::tempdir().unwrap();
        let sender = MockSender::new(Reason::Sent);
        let responses = serve_lines(
            tmp.path(),
            &env_of(&[]),
            &sender,
            concat!(
                "\n",
                "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
                "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n",
            ),
        );
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["id"], 7);
    }

    /// An id-bearing `notifications/*` line is not a notification: it is a
    /// request for a method this server does not serve, refused like any
    /// other unknown method.
    #[test]
    fn an_id_bearing_notification_is_an_unknown_method() {
        let tmp = tempfile::tempdir().unwrap();
        let sender = MockSender::new(Reason::Sent);
        let responses = serve_lines(
            tmp.path(),
            &env_of(&[]),
            &sender,
            "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"notifications/initialized\"}\n",
        );
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["id"], 8);
        assert_eq!(responses[0]["error"]["code"], METHOD_NOT_FOUND);
        assert!(
            responses[0]["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("notifications/initialized")),
            "the refusal names the method: {}",
            responses[0]["error"]["message"]
        );
    }
}
