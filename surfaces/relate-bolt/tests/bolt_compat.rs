//! The Bolt compatibility corpus (ADR-0304): every boltkit-format `.bolt`
//! script under `tests/compat/scripts` (recursively) is replayed against
//! its own real `relate-bolt` process, the harness playing the client.
//! `tests/compat/README.md` is the script reference.

mod common;
mod compat;

use std::path::PathBuf;

fn compat_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/compat")
}

/// The corpus: one test for every script, failing with a list of every
/// failed script line.
#[test]
fn every_compat_script_passes() {
    let paths = compat::discover(&compat_dir().join("scripts"));
    let failures = compat::run_scripts(&paths);
    assert!(
        failures.is_empty(),
        "{} of {} Bolt compat scripts failed:\n\n{}",
        failures.len(),
        paths.len(),
        failures
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}

/// The README's example is a corpus script, byte for byte, so the
/// documented example runs on every pass.
#[test]
fn readme_example_is_a_corpus_script() {
    let readme = std::fs::read_to_string(compat_dir().join("README.md")).expect("read README.md");
    let example = readme
        .split_once("```text\n# The README example")
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(block, _)| format!("# The README example{block}"))
        .expect("README.md carries the example block");
    let script = std::fs::read_to_string(compat_dir().join("scripts/readme_example.bolt"))
        .expect("read scripts/readme_example.bolt");
    assert_eq!(example, script, "README.md example drifted from the script");
}

/// The failure report names the script and the line of the wrong
/// expectation, quotes that line, and shows the decoded actual message.
/// The fixture lives outside `scripts/`, so the corpus never runs it.
#[test]
fn a_mismatch_names_the_script_line() {
    let fixture = compat_dir().join("fixtures/mismatch.bolt");
    let source = std::fs::read_to_string(&fixture).expect("read the mismatch fixture");
    let wrong = r#"S: SUCCESS {"fields": ["y"]}"#;
    let line = 1 + source
        .lines()
        .position(|l| l.trim() == wrong)
        .expect("the fixture carries the wrong expectation");

    let failures = compat::run_scripts(std::slice::from_ref(&fixture));
    assert_eq!(
        failures.len(),
        1,
        "the deliberately wrong fixture must fail"
    );
    let report = failures[0].to_string();

    let location = format!("{}:{line}:", fixture.display());
    assert!(
        report.contains(&location),
        "missing {location} in:\n{report}"
    );
    assert!(
        report.contains(wrong),
        "missing the expected line in:\n{report}"
    );
    assert!(
        report.contains(r#"actual:   SUCCESS {"#) && report.contains(r#""fields": ["x"]"#),
        "missing the decoded actual message in:\n{report}"
    );
}
