//! Signal primitives shared by both binaries of this crate (the server and
//! the container entrypoint — included there via `#[path]`). Each binary
//! logs its own message after the wait. ADR-0233: signal-hook's blocking
//! iterator replaces tokio's signal module; no runtime is involved. The
//! wait and its install-failure policy are the workspace's one
//! implementation, [`relate_serve::signal`] (ADR-0238), which both
//! binaries call directly.

/// The conventional shell exit code for a terminating signal: 128 + signal.
pub fn signal_exit_code(sig: i32) -> i32 {
    128 + sig
}

/// The log name of a shutdown signal.
pub fn signal_name(sig: i32) -> &'static str {
    match sig {
        signal_hook::consts::SIGINT => "SIGINT",
        signal_hook::consts::SIGTERM => "SIGTERM",
        // Only SIGINT/SIGTERM are registered; an unregistered number must
        // never be mislabeled as one of them.
        _ => "unknown signal",
    }
}
