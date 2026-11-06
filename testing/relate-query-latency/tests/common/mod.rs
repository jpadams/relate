//! Shared helpers of the crate's integration tests.

/// Whether a gated test runs: its env var must be `1`.
pub fn enabled(var: &str, what: &str) -> bool {
    if std::env::var(var).as_deref() == Ok("1") {
        return true;
    }
    eprintln!("skipped: set {var}=1 to run {what}");
    false
}
