//! End-to-end tests for the `envelope` output mode (ADR-0195).

#[allow(dead_code)]
mod support;

use support::{run_cli, CliRunOptions as Opts};

#[test]
fn test_envelope_mode_prints_columns_rows_and_truncated() {
    let (status, stdout, stderr) = run_cli(Opts::new(&[
        "--mode",
        "envelope",
        "RETURN 1 AS a, [1,2] AS l",
    ]));
    assert!(status.success(), "{stderr}");
    assert_eq!(
        stdout,
        concat!(
            "{\"columnTypes\":[\"INTEGER\",\"LIST\"],\"columns\":[\"a\",\"l\"],",
            "\"notifications\":[],\"rows\":[{\"a\":1,\"l\":[1,2]}],\"truncated\":false}\n"
        )
    );
}

#[test]
fn test_envelope_mode_keeps_columns_for_an_empty_result() {
    let (status, stdout, stderr) = run_cli(Opts::new(&[
        "--mode",
        "envelope",
        "MATCH (n:Nope) RETURN n.p AS p",
    ]));
    assert!(status.success(), "{stderr}");
    assert_eq!(
        stdout,
        "{\"columnTypes\":[\"ANY\"],\"columns\":[\"p\"],\"notifications\":[],\"rows\":[],\"truncated\":false}\n"
    );
}

#[test]
fn test_envelope_mode_prints_one_line_per_statement() {
    let (status, stdout, _) = run_cli(Opts::new(&[
        "--mode",
        "envelope",
        "RETURN 1 AS a; RETURN 2 AS b",
    ]));
    assert!(status.success());
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "stdout: {stdout}");
    for (line, col) in lines.iter().zip(["a", "b"]) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["columns"][0], col);
    }
}

#[test]
fn test_envelope_mode_failure_carries_the_error_envelope() {
    let (status, stdout, _) = run_cli(Opts::new(&["--mode", "envelope", "RETRN 1"]));
    assert_eq!(status.code(), Some(6));
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["error"]["code"], "validation_error");
}

#[test]
fn test_json_and_jsonl_modes_are_unchanged_by_the_envelope_mode() {
    // Piped stdout renders compact JSON (ADR-0240).
    let (_, json, _) = run_cli(Opts::new(&["--json", "RETURN 1 AS a"]));
    assert_eq!(json, "[{\"a\":1}]\n");
    let (_, jsonl, _) = run_cli(Opts::new(&["--jsonl", "RETURN 1 AS a"]));
    assert_eq!(jsonl, "{\"a\":1}\n");
}

#[test]
fn test_invalid_mode_lists_envelope_among_the_choices() {
    let (status, _, stderr) = run_cli(Opts::new(&["--mode", "bogus", "RETURN 1"]));
    assert_eq!(status.code(), Some(2));
    assert!(
        stderr
            .contains("expected one of: table, csv, json, jsonl, envelope, markdown, cypher, toon"),
        "stderr: {stderr}"
    );
}
