mod boundaries;

use std::process::{Command, ExitCode};

const HELP: &str = "\
Workspace maintenance tasks for Relate.

Usage: cargo xtask <COMMAND> [OPTIONS]

Commands:
  check-boundaries  Check workspace dependency edges against ADR-0103 rules R1-R5, R7

Options:
  -h, --help  Print help
";

const CHECK_BOUNDARIES_HELP: &str = "\
Check workspace dependency edges against ADR-0103 rules R1-R5 and the
extensions-group placement rules R7 (ADR-0431 §11).

Reads `cargo metadata --format-version 1 --no-deps` (using $CARGO if set,
else `cargo`), classifies every workspace member by its directory
(relate/<pkg>, surfaces/<pkg>, testing/<pkg>, extensions/<pkg>) and
checks every workspace-internal dependency edge by kind (normal, dev,
build).

Usage: cargo xtask check-boundaries [OPTIONS]

Options:
  -h, --help  Print help

Exit status:
  0  no violations
  1  one or more violations, each printed as
     `<from-pkg> -> <to-pkg> (<dep-kind>): <rule id>` or
     `<pkg> (<dir>): R5`
  2  usage error or `cargo metadata` failure
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some("check-boundaries") => match args.get(1).map(String::as_str) {
            None => check_boundaries(),
            Some("-h" | "--help") => {
                print!("{CHECK_BOUNDARIES_HELP}");
                ExitCode::SUCCESS
            }
            Some(other) => usage_error(&format!("unexpected argument '{other}'")),
        },
        Some(other) => usage_error(&format!("unknown command '{other}'")),
        None => usage_error("missing command"),
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("error: {message}\n\n{HELP}");
    ExitCode::from(2)
}

fn check_boundaries() -> ExitCode {
    let packages = match load_metadata().and_then(|m| boundaries::parse_metadata(&m)) {
        Ok(packages) => packages,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let report = boundaries::check(&packages);
    let summary = format!(
        "check-boundaries: {} packages, {} edges checked, {} violations",
        report.packages,
        report.edges,
        report.violations.len()
    );
    if report.violations.is_empty() {
        println!("{summary}");
        return ExitCode::SUCCESS;
    }
    for violation in &report.violations {
        eprintln!("{violation}");
    }
    eprintln!("{summary}");
    ExitCode::FAILURE
}

fn load_metadata() -> Result<serde_json::Value, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("failed to run `{cargo} metadata`: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{cargo} metadata` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("invalid cargo metadata: {e}"))
}
