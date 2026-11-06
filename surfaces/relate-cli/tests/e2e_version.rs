//! End-to-end tests for the CLI's `--version` output: the one-object JSON
//! shape and the two-line text shape (ADR-0193, ADR-0432). Split from
//! `e2e_cli.rs` so the version surface has its own home.

#[allow(dead_code)]
mod support;

use support::{run_cli, CliRunOptions as Opts};

#[test]
fn test_version_json_is_one_compact_object() {
    for args in [
        &["--version", "--json"][..],
        &["--json", "--version"],
        &["--version", "--mode", "json"],
        &["-version", "-json"],
    ] {
        let (status, stdout, stderr) = run_cli(Opts::new(args));
        assert!(status.success(), "{args:?} exited non-zero: {stderr}");
        assert!(stderr.is_empty(), "{args:?} stderr: {stderr}");
        assert_eq!(stdout.lines().count(), 1, "{args:?} stdout: {stdout}");
        let v: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("{args:?} not JSON ({e}): {stdout}"));
        assert_eq!(v["name"], "relate");
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(v["os"], std::env::consts::OS);
        assert_eq!(v["arch"], std::env::consts::ARCH);
        assert_eq!(v["license"], "Apache 2.0 (binary)");
    }
}

#[test]
fn test_version_text_prints_version_and_license_lines() {
    // Two lines in every mode but json (ADR-0432), across the --version,
    // -version, and -v spellings.
    let expected = format!(
        "Relate v{}\nLicense: Apache 2.0 (binary)\n",
        env!("CARGO_PKG_VERSION")
    );
    for args in [
        &["--version"][..],
        &["--version", "--csv"],
        &["--version", "--jsonl"],
        &["-version"],
        &["-v"],
    ] {
        let (status, stdout, stderr) = run_cli(Opts::new(args));
        assert!(status.success(), "{args:?} exited non-zero: {stderr}");
        assert!(stderr.is_empty(), "{args:?} stderr: {stderr}");
        assert_eq!(stdout, expected, "{args:?}");
    }
}

#[test]
fn test_still_set_cypherlite_env_name_fails_closed() {
    // ADR-0517: the CYPHERLITE_* names were renamed to RELATE_*; a still-set
    // old name must not resolve as unset (unset is the unlimited/default
    // arm for the limit and safety knobs), so the CLI fails closed naming
    // both spellings, and the new spelling alone keeps working.
    let (status, _, stderr) =
        run_cli(Opts::new(&["--version"]).with_envs(&[("CYPHERLITE_PREVIEW", "hybrid-search")]));
    assert!(
        !status.success(),
        "exited successfully with a set old name: {stderr}"
    );
    assert!(
        stderr.contains("CYPHERLITE_PREVIEW is set but ignored; set RELATE_PREVIEW"),
        "{stderr}"
    );
    let (status, stdout, stderr) =
        run_cli(Opts::new(&["--version"]).with_envs(&[("RELATE_PREVIEW", "hybrid-search")]));
    assert!(status.success(), "new spelling rejected: {stderr}");
    assert!(stdout.contains("Relate v"), "{stdout}");
}

#[test]
fn test_cypherlite_env_guard_casing_and_double_renames() {
    // The prefix test is case-insensitive (Windows env lookup is), so a
    // mixed-case old name fails closed under the canonical spelling, and
    // the three knobs renamed twice are guided to their current spelling,
    // not the mechanical intermediate the engine also rejects.
    let (status, _, stderr) =
        run_cli(Opts::new(&["--version"]).with_envs(&[("cYpHeRlItE_pReViEw", "hybrid-search")]));
    assert!(!status.success(), "mixed-case old name ignored: {stderr}");
    assert!(
        stderr.contains("CYPHERLITE_PREVIEW is set but ignored; set RELATE_PREVIEW"),
        "{stderr}"
    );
    let (status, _, stderr) =
        run_cli(Opts::new(&["--version"]).with_envs(&[("CYPHERLITE_TOKEN_ADMISSION", "100")]));
    assert!(
        !status.success(),
        "double-renamed old name ignored: {stderr}"
    );
    assert!(
        stderr.contains(
            "CYPHERLITE_TOKEN_ADMISSION is set but ignored; \
             set RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT"
        ),
        "{stderr}"
    );
}
