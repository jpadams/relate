//! The one shutdown-signal wait every Relate network surface (and the
//! CLI's `serve`) blocks on: signal-hook's blocking iterator on unix
//! (SIGINT and SIGTERM, ADR-0233); a handler-signalled channel receive
//! elsewhere, because signal-hook's `iterator` module is
//! `not(windows)`-gated and Windows consoles deliver only Ctrl-C (SIGINT).
//! The caller's main thread owns the shutdown, so teardown (a final
//! checkpoint, child reaping) runs off the signal context.

/// Block until a shutdown signal arrives and return the raw signal number;
/// callers log per signal and exit 130/143 (ADR-0264). The error is the
/// handler-installation failure.
#[cfg(unix)]
pub fn wait_shutdown_signal() -> std::io::Result<i32> {
    use signal_hook::consts::{SIGINT, SIGTERM};
    let mut signals = signal_hook::iterator::Signals::new([SIGINT, SIGTERM])?;
    Ok(signals.forever().next().expect("signal stream ended"))
}

/// See the unix variant. signal-hook's blocking `iterator` module is
/// `not(windows)`-gated, so the wait blocks on a condvar the registered
/// action notifies — no polling, no lost wakeup.
#[cfg(not(unix))]
pub fn wait_shutdown_signal() -> std::io::Result<i32> {
    use std::sync::{Arc, Condvar, Mutex};

    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let notified = Arc::clone(&pair);
    // SAFETY: Windows runs the console-control handler on its own thread
    // (the CRT's `signal` machinery), so locking a mutex and notifying a
    // condvar inside the action is safe there; this variant never compiles
    // on unix, where the action would run inside the async signal handler.
    // The action never panics (a poisoned lock is simply not taken).
    unsafe {
        signal_hook::low_level::register(signal_hook::consts::SIGINT, move || {
            let (lock, cvar) = &*notified;
            if let Ok(mut hit) = lock.lock() {
                *hit = true;
                cvar.notify_all();
            }
        })?;
    }
    let (lock, cvar) = &*pair;
    let mut hit = lock.lock().expect("shutdown flag mutex poisoned");
    while !*hit {
        hit = cvar.wait(hit).expect("shutdown flag mutex poisoned");
    }
    Ok(signal_hook::consts::SIGINT)
}

/// The wait-or-exit policy every surface's signal thread runs: block in
/// [`wait_shutdown_signal`]; a handler-installation failure is reported
/// with `prefix` and exits 1 — a server that cannot watch for its shutdown
/// signal must not run unwatched. One home, so the bolt server, the fleet
/// entrypoint, and the CLI's `serve` cannot drift in wording or policy.
pub fn wait_or_exit(prefix: &str) -> i32 {
    match wait_shutdown_signal() {
        Ok(sig) => sig,
        Err(e) => {
            eprintln!("{prefix}: could not install the signal handlers: {e}");
            std::process::exit(1);
        }
    }
}
