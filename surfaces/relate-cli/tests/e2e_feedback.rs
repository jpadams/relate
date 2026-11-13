//! End-to-end tests for the `feedback` positional lifecycle command
//! (ADR-0069 grammar, ADR-0588 §3): `relate feedback
//! submit|enable|disable|status`.
//!
//! Executes the actual `relate` binary against a fake `$HOME` (tempdir) so
//! the consent record and the receipts live in an isolated
//! `$HOME/.config/relate/`, and against the shared loopback Mixpanel mock
//! (`MockMixpanel`, the telemetry suite's capture sink): the report POST is
//! synchronous, so the tests assert the captured event directly and that a
//! first-run (no consent), disabled, opted-out or rejected report sends
//! nothing at all.

#![cfg(feature = "feedback")]

mod support;

use std::path::Path;
use std::time::Duration;

use support::{run_cli, tempdir, CliRunOptions as Opts, MockMixpanel, QUIET_GRACE};

const SUBMIT_ARGS: [&str; 10] = [
    "feedback",
    "submit",
    "--category",
    "unclear_error",
    "--expected",
    "the info command to print the version",
    "--observed",
    "it printed help instead",
    "--command",
    "info",
];

/// The feedback suite's one spawn shape: the isolated home and the mock sink,
/// with the product's telemetry layers deciding (default on).
fn opts<'a>(home: &'a Path, url: &'a str, args: &'a [&'a str]) -> Opts<'a> {
    Opts::new(args).in_home(home).with_feedback_endpoint(url)
}

/// The persisted feedback consent block from the fake home's config.json.
fn config_feedback(home: &Path) -> Option<serde_json::Value> {
    let path = home.join(".config").join("relate").join("config.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("feedback").cloned()
}

/// The receipt log's lines, empty when no receipt was written.
fn receipts(home: &Path) -> Vec<serde_json::Value> {
    let path = home
        .join(".config")
        .join("relate")
        .join("feedback-receipts.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| panic!("non-JSON receipt: {e}: {line}"))
        })
        .collect()
}

fn submit_outcome(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("submit must print one JSON outcome line: {e}: {stdout}"))
}

/// The first run carries no consent record. The shell path never asks
/// (ADR-0588 §3): consent is the explicit `feedback enable` step, so until a
/// person runs it a submit is a local `skipped`/`not_enabled` that sends
/// nothing and writes no receipt.
#[test]
fn test_first_run_without_consent_is_a_local_skip() {
    let home = tempdir();
    let mock = MockMixpanel::start();
    let url = mock.url();

    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &["feedback", "status"]));
    assert_eq!(status.code(), Some(0), "status: {stderr}");
    assert!(
        stdout.starts_with("disabled (not enabled)"),
        "got: {stdout} {stderr}"
    );

    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &SUBMIT_ARGS));
    assert_eq!(status.code(), Some(0), "consentless submit: {stderr}");
    let doc = submit_outcome(&stdout);
    assert_eq!(doc["outcome"], "skipped", "{doc}");
    assert_eq!(doc["reason"], "not_enabled", "{doc}");
    std::thread::sleep(QUIET_GRACE);
    assert_eq!(
        mock.request_count(),
        0,
        "without consent a report is never sent"
    );
    assert!(
        receipts(home.path()).is_empty(),
        "a consent skip writes no receipt"
    );

    // The consent step is the plain `feedback enable`; it records the
    // authorization the next submit clears.
    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &["feedback", "enable"]));
    assert_eq!(status.code(), Some(0), "enable: {stderr}");
    assert!(
        stdout.contains("feedback enabled until"),
        "got: {stdout} {stderr}"
    );
    let feedback = config_feedback(home.path()).expect("enable writes the consent record");
    assert!(
        feedback["authorized-until"].is_string(),
        "the consent record carries a deadline: {feedback}"
    );
}

#[test]
fn test_enable_submit_status_disable_roundtrip() {
    let home = tempdir();
    let mock = MockMixpanel::start();
    let url = mock.url();

    // enable: the consent record is written.
    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &["feedback", "enable"]));
    assert_eq!(status.code(), Some(0), "enable: {stderr}");
    assert!(
        stdout.contains("feedback enabled until"),
        "got: {stdout} {stderr}"
    );
    let feedback = config_feedback(home.path()).expect("enable writes the feedback record");
    assert!(
        feedback["authorized-until"].is_string(),
        "authorization must carry a deadline: {feedback}"
    );

    // submit: one report reaches the mock endpoint and is receipted locally.
    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &SUBMIT_ARGS));
    assert_eq!(status.code(), Some(0), "submit: {stderr} {stdout}");
    let doc = submit_outcome(&stdout);
    assert_eq!(doc["outcome"], "sent", "{doc}");
    assert_eq!(doc["reason"], "sent", "{doc}");
    assert!(
        mock.wait_for_events(Duration::from_secs(5), |events| events.iter().any(|e| {
            e["event"] == "RELATE-CLI_AGENT_FEEDBACK"
                && e["properties"]["expected"] == "the info command to print the version"
                && e["properties"]["observed"] == "it printed help instead"
                && e["properties"]["category"] == "unclear_error"
        })),
        "the report never reached the sink; captured: {:?}",
        mock.events()
    );
    let lines = receipts(home.path());
    assert_eq!(lines.len(), 1, "one receipt per send: {lines:?}");
    assert_eq!(lines[0]["outcome"], "sent", "{lines:?}");
    assert_eq!(lines[0]["request_id"], doc["request_id"], "{lines:?}");
    assert_eq!(
        lines[0]["payload"]["expected"], "the info command to print the version",
        "{lines:?}"
    );

    // status: reports the live authorization.
    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &["feedback", "status"]));
    assert_eq!(status.code(), Some(0), "status: {stderr}");
    assert!(
        stdout.starts_with("enabled (until "),
        "got: {stdout} {stderr}"
    );

    // disable: the authorization is revoked.
    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &["feedback", "disable"]));
    assert_eq!(status.code(), Some(0), "disable: {stderr}");
    assert!(stdout.contains("feedback disabled"), "got: {stdout}");
    assert_eq!(
        config_feedback(home.path()),
        Some(serde_json::json!({})),
        "disable clears the consent record"
    );

    // A disabled submit is a local skip: exit 0, and nothing is sent.
    let sent_before = mock.request_count();
    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &SUBMIT_ARGS));
    assert_eq!(status.code(), Some(0), "disabled submit: {stderr} {stdout}");
    let doc = submit_outcome(&stdout);
    assert_eq!(doc["outcome"], "skipped", "{doc}");
    assert_eq!(doc["reason"], "not_enabled", "{doc}");
    std::thread::sleep(QUIET_GRACE);
    assert_eq!(
        mock.request_count(),
        sent_before,
        "a disabled submit must send nothing"
    );
}

/// `DO_NOT_TRACK=1` suppresses the send even with a recorded authorization:
/// the report is a local `skipped`/`do_not_track`. The same report without
/// the opt-out reaches the sink, so the env layer is what decided.
#[test]
fn test_do_not_track_suppresses_the_send() {
    let home = tempdir();
    let mock = MockMixpanel::start();
    let url = mock.url();
    let (status, _, stderr) = run_cli(opts(home.path(), &url, &["feedback", "enable"]));
    assert_eq!(status.code(), Some(0), "enable: {stderr}");

    let opt_out = [("DO_NOT_TRACK", "1")];
    let (status, stdout, stderr) = run_cli(
        Opts::new(&SUBMIT_ARGS)
            .in_home(home.path())
            .with_feedback_endpoint(&url)
            .with_envs(&opt_out),
    );
    assert_eq!(status.code(), Some(0), "opted-out submit: {stderr}");
    let doc = submit_outcome(&stdout);
    assert_eq!(doc["outcome"], "skipped", "{doc}");
    assert_eq!(doc["reason"], "do_not_track", "{doc}");
    std::thread::sleep(QUIET_GRACE);
    assert_eq!(mock.request_count(), 0, "DO_NOT_TRACK must send nothing");
    assert!(
        receipts(home.path()).is_empty(),
        "an opt-out skip writes no receipt"
    );

    // The same report without the opt-out reaches the sink.
    let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &SUBMIT_ARGS));
    assert_eq!(status.code(), Some(0), "submit: {stderr} {stdout}");
    assert_eq!(submit_outcome(&stdout)["outcome"], "sent", "{stdout}");
    assert!(
        mock.wait_for_events(Duration::from_secs(5), |events| events
            .iter()
            .any(|e| e["event"] == "RELATE-CLI_AGENT_FEEDBACK")),
        "the report never reached the sink; captured: {:?}",
        mock.events()
    );
}

/// A report the policy refuses never leaves the machine: exit 1 with the
/// outcome document naming the field and detector, no POST, and one receipt
/// that records no payload text. Both rejection classes are covered: an
/// input-shape fault (`invalid_input`) and a redaction hit
/// (`policy_violation`).
#[test]
fn test_an_invalid_report_is_rejected_locally_with_its_exit_code_and_message() {
    let home = tempdir();
    let mock = MockMixpanel::start();
    let url = mock.url();

    let (status, _, stderr) = run_cli(opts(home.path(), &url, &["feedback", "enable"]));
    assert_eq!(status.code(), Some(0), "enable: {stderr}");

    let cases: [([&str; 8], serde_json::Value); 2] = [
        (
            [
                "feedback",
                "submit",
                "--category",
                "not_a_category",
                "--expected",
                "an error",
                "--observed",
                "the run failed",
            ],
            serde_json::json!({"field": "category", "detector": "invalid_input"}),
        ),
        (
            [
                "feedback",
                "submit",
                "--category",
                "other_friction",
                "--expected",
                "an error",
                "--observed",
                "it mentioned dev@example.com",
            ],
            serde_json::json!({"field": "observed", "detector": "email"}),
        ),
    ];
    for (args, violation) in &cases {
        let (status, stdout, stderr) = run_cli(opts(home.path(), &url, &args[..]));
        assert_eq!(status.code(), Some(1), "{args:?}: {stderr}");
        let doc = submit_outcome(&stdout);
        assert_eq!(doc["outcome"], "rejected", "{args:?}: {doc}");
        assert_eq!(&doc["violations"][0], violation, "{args:?}: {doc}");
        assert!(
            doc["payload"].is_null(),
            "{args:?}: a rejected report has no payload: {doc}"
        );
    }
    std::thread::sleep(QUIET_GRACE);
    assert_eq!(mock.request_count(), 0, "a rejected report is never sent");

    let lines = receipts(home.path());
    assert_eq!(
        lines.len(),
        cases.len(),
        "one receipt per rejection: {lines:?}"
    );
    for line in lines {
        assert_eq!(line["outcome"], "rejected", "{line}");
        assert!(
            line["payload"].is_null(),
            "a rejection receipt records no report text: {line}"
        );
    }
}

/// An unusable destination — a refused `RELATE_FEEDBACK_ENDPOINT` (https on a
/// non-Mixpanel host) and no destination at all — is one contract: `enable`
/// exits 1, records nothing, and `status` therefore reports not enabled
/// rather than an authorization every later submit could only report
/// `not_configured` for.
#[test]
fn test_an_unusable_destination_is_refused_and_not_recorded() {
    let home = tempdir();
    let cases: [(&[(&str, &str)], &str); 2] = [
        (
            &[("RELATE_FEEDBACK_ENDPOINT", "https://sink.example/track")],
            "RELATE_FEEDBACK_ENDPOINT=https://sink.example/track",
        ),
        (&[], "no destination is configured"),
    ];
    for (envs, refusal) in cases {
        let (status, _, stderr) = run_cli(
            Opts::new(&["feedback", "enable"])
                .in_home(home.path())
                .with_default_telemetry()
                .with_envs(envs),
        );
        assert_eq!(
            status.code(),
            Some(1),
            "{refusal}: enable must refuse: {stderr}"
        );
        assert!(
            stderr.contains(refusal),
            "{refusal}: the refusal names why: {stderr}"
        );
        assert!(
            config_feedback(home.path()).is_none(),
            "{refusal}: no destination-less authorization is recorded"
        );

        let (status, stdout, stderr) = run_cli(
            Opts::new(&["feedback", "status"])
                .in_home(home.path())
                .with_default_telemetry(),
        );
        assert_eq!(status.code(), Some(0), "{refusal}: status: {stderr}");
        assert!(
            stdout.starts_with("disabled (not enabled)"),
            "{refusal}: status reflects the refused enable: {stdout}"
        );
    }
}

#[test]
fn test_usage_errors_exit_2() {
    let home = tempdir();
    for args in [
        &["feedback"][..],
        &["feedback", "frobnicate"][..],
        &["feedback", "status", "extra"][..],
        &["feedback", "submit"][..],
        &[
            "feedback",
            "submit",
            "--category",
            "unclear_error",
            "--expected",
            "x",
            "--observed",
            "y",
            "--bogus",
            "z",
        ][..],
    ] {
        let (status, _, stderr) = run_cli(Opts::new(args).in_home(home.path()));
        assert_eq!(status.code(), Some(2), "{args:?}: {stderr}");
        assert!(
            stderr.contains("Usage: relate feedback"),
            "{args:?}: {stderr}"
        );
    }
}
