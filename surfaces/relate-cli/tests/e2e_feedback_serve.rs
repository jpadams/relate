//! End-to-end tests for the hidden `relate feedback serve` MCP stdio server
//! (ADR-0588 §4): one tool, `feedback_submit`, spoken as newline-delimited
//! JSON-RPC 2.0 on the process's own stdin/stdout.
//!
//! The first call carries no consent and reports the same skip the submit
//! pipeline produces; the second runs against a recorded authorization and
//! a loopback mock sink, so the receipt is a real send.

#![cfg(feature = "feedback")]

mod support;

use std::path::{Path, PathBuf};

use relate_feedback::authorization::FeedbackConsent;
use support::{run_cli, tempdir, CliRunOptions as Opts, MockMixpanel};

/// The config directory a run under the fake `$HOME` resolves.
fn config_dir(home: &Path) -> PathBuf {
    home.join(".config").join("relate")
}

/// Record a live authorization for `endpoint`, through the crate's own
/// consent record: exactly the state `feedback enable` writes, window
/// included.
fn authorize(home: &Path, endpoint: &str) {
    let dir = config_dir(home);
    std::fs::create_dir_all(&dir).unwrap();
    let mut record = FeedbackConsent::default();
    record.authorize(chrono::Utc::now(), endpoint.to_string());
    record.save(&dir).unwrap();
}

fn call_line(id: u64) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"feedback_submit\",\"arguments\":{{\"category\":\"unclear_error\",\"expected\":\"the info command to print the version\",\"observed\":\"it printed help instead\"}}}}}}"
    )
}

/// The server's answer lines, one parsed JSON value each.
fn responses(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("non-JSON line on stdout: {error}: {line}"))
        })
        .collect()
}

/// The outcome document one `tools/call` response embeds, plus its `isError`.
fn tool_result(response: &serde_json::Value) -> (serde_json::Value, bool) {
    let result = &response["result"];
    let is_error = result["isError"].as_bool().unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    (serde_json::from_str(text).unwrap(), is_error)
}

#[test]
fn test_serve_lists_one_tool_and_reports_the_unconsented_skip() {
    let home = tempdir();
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
    )
    .to_string()
        + &call_line(3);
    let (status, stdout, stderr) = run_cli(
        Opts::new(&["feedback", "serve"])
            .in_home(home.path())
            .with_default_telemetry()
            .with_stdin(input.as_bytes()),
    );
    assert!(status.success(), "serve failed: {stderr}");
    let out = responses(&stdout);
    assert_eq!(out.len(), 3, "one line per request: {stdout}");
    assert_eq!(out[0]["result"]["serverInfo"]["name"], "relate-feedback");
    let tools = out[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "feedback_submit");
    let (outcome, is_error) = tool_result(&out[2]);
    assert_eq!(outcome["outcome"], "skipped");
    assert_eq!(outcome["reason"], "not_enabled");
    assert!(!is_error, "a skip is not a tool error");
    assert!(outcome["receipt_path"].is_null());
    assert!(!config_dir(home.path())
        .join("feedback-receipts.jsonl")
        .exists());
}

#[test]
fn test_serve_sends_with_recorded_consent_and_receipts_it() {
    let home = tempdir();
    let recorded = MockMixpanel::start();
    let recorded_endpoint = format!("{}/track", recorded.url());
    authorize(home.path(), &recorded_endpoint);
    let diverted = MockMixpanel::start();
    let diverted_endpoint = format!("{}/track", diverted.url());
    let envs = [("RELATE_FEEDBACK_ENDPOINT", diverted_endpoint.as_str())];
    let (status, stdout, stderr) = run_cli(
        Opts::new(&["feedback", "serve"])
            .in_home(home.path())
            .with_default_telemetry()
            .with_envs(&envs)
            .with_stdin(call_line(1).as_bytes()),
    );
    assert!(status.success(), "serve failed: {stderr}");
    let out = responses(&stdout);
    assert_eq!(out.len(), 1, "{stdout}");
    let (outcome, is_error) = tool_result(&out[0]);
    assert_eq!(outcome["outcome"], "sent", "{outcome}");
    assert_eq!(outcome["reason"], "sent");
    assert!(!is_error);
    let receipt = outcome["receipt_path"].as_str().unwrap();
    assert_eq!(
        Path::new(receipt),
        config_dir(home.path()).join("feedback-receipts.jsonl")
    );
    assert!(Path::new(receipt).exists(), "the send is receipted locally");

    let events = recorded.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        diverted.request_count(),
        0,
        "RELATE_FEEDBACK_ENDPOINT cannot divert a recorded consent"
    );
    assert_eq!(events[0]["event"], "RELATE-CLI_AGENT_FEEDBACK");
    assert_eq!(
        events[0]["properties"]["expected"],
        "the info command to print the version"
    );
    assert_eq!(events[0]["properties"]["category"], "unclear_error");
    assert_eq!(events[0]["properties"]["surface"], "cli");
}
