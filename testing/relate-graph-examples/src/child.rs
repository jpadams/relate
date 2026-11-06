//! The child-process lifecycle subsystem (F-1-1: one module, two call
//! sites — [`docker`]'s admin load and [`pull`]'s CLI run): pipe draining,
//! deadline polling, kill-and-reap, and the shared wait ceremony.

/// Drain one child-process pipe on its own thread; the joined result is
/// the pipe's text. Shared by the process-spawning modules ([`crate::docker`]'s
/// admin load and [`crate::pull`]'s CLI run — three call sites, one
/// definition): a chatty child can never block on a full pipe. Callers
/// pass the pipe by value — every call site sets `Stdio::piped()` right
/// before the `.take()`, so a missing pipe is a bug, not a mode
/// (`.expect("… is piped")` at the call site makes that invariant
/// explicit).
pub(crate) fn read_to_string(
    mut pipe: impl std::io::Read + Send + 'static,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

/// How a [`wait_with_deadline`] poll ended.
pub(crate) enum WaitOutcome {
    /// The child exited on its own with this status.
    Exited(std::process::ExitStatus),
    /// The deadline passed first: the child was killed and reaped.
    TimedOut,
}

/// Kill a still-running child and reap it, so a timeout or poll error
/// never orphans the process (F-1-12: one definition — [`crate::pull`]'s
/// CLI run and [`crate::docker`]'s admin load both go through here).
pub(crate) fn kill_and_wait(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Poll `child` until it exits or `deadline` passes (F-1-2: the one
/// try_wait-until-deadline loop both process-spawning modules share — the
/// two copies had already drifted on poll interval, deadline comparison,
/// and poll-error cleanup). Past the deadline the child is killed and
/// reaped and the result is [`WaitOutcome::TimedOut`]; a poll error also
/// kills and reaps — never orphan a still-running child — and returns the
/// error. Pipes are drained by the caller's [`read_to_string`] reader
/// threads, which the caller joins after this returns.
pub(crate) fn wait_with_deadline(
    child: &mut std::process::Child,
    deadline: std::time::Instant,
) -> std::io::Result<WaitOutcome> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(WaitOutcome::Exited(status)),
            Ok(None) if std::time::Instant::now() >= deadline => {
                kill_and_wait(child);
                return Ok(WaitOutcome::TimedOut);
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            // A poll error leaves the child's state unknown, so kill and
            // reap before returning the error — never orphan a
            // still-running child. This arm restores the
            // pre-consolidation pull.rs behavior (F-3-1); a poll error
            // cannot be forced cheaply in a test (a reaped child's stored
            // status keeps `try_wait` succeeding), so the contract lives
            // here instead.
            Err(e) => {
                kill_and_wait(child);
                return Err(e);
            }
        }
    }
}

/// The whole wait ceremony both process-spawning modules share
/// (F-3-3/F-3-4: one definition, two call sites): poll `child` until exit
/// or `deadline` via [`wait_with_deadline`], join the stderr reader
/// exactly once — here — and report failures in the call site's words. A
/// poll error becomes `"{poll_error_context}: {e}"`; a timeout becomes
/// `timeout_message(stderr)` built from the drained stderr. On success
/// the exit status and the drained stderr come back together, so the
/// caller never joins the reader itself.
pub(crate) fn wait_with_deadline_or_timeout(
    child: &mut std::process::Child,
    deadline: std::time::Instant,
    stderr_reader: std::thread::JoinHandle<String>,
    poll_error_context: &str,
    timeout_message: impl FnOnce(&str) -> String,
) -> Result<(std::process::ExitStatus, String), String> {
    let outcome = wait_with_deadline(child, deadline);
    // The child exited, was killed, or — on a poll error — was killed
    // and reaped by `wait_with_deadline`'s own error arm, so on every
    // path its stderr pipe is at EOF and the reader thread finishes:
    // this one join — the reader's only join, taken before the outcome
    // is classified so no path drops the handle — never blocks. A
    // panicked reader surfaces here instead of self-hiding as a
    // fabricated-empty stderr (pull.rs's stdout reader discipline,
    // F-3-3/F-3-4's sibling contract).
    let stderr = stderr_reader
        .join()
        .expect("the stderr reader thread finishes");
    match outcome {
        Ok(WaitOutcome::Exited(status)) => Ok((status, stderr)),
        Ok(WaitOutcome::TimedOut) => Err(timeout_message(&stderr)),
        Err(e) => Err(format!("{poll_error_context}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    /// After a deadline kill the child's stored status is immediately
    /// available (already reaped — no live child to wait on) and shows the
    /// kill, not a clean exit. Shared by both timeout tests (F-2-2/F-2-3:
    /// one definition, two call sites).
    fn assert_reaped_as_killed(child: &mut std::process::Child) {
        let status = child
            .try_wait()
            .expect("a reaped child keeps its stored status")
            .expect("the status is stored after the reap");
        assert!(!status.success(), "killed by the deadline: {status}");
    }

    /// A child that exits before the deadline comes back as `Exited` with
    /// its real status.
    #[test]
    fn wait_with_deadline_returns_exited_for_a_fast_child() {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        match super::wait_with_deadline(&mut child, deadline).expect("poll") {
            super::WaitOutcome::Exited(status) => assert!(status.success()),
            super::WaitOutcome::TimedOut => panic!("a fast child must exit, not time out"),
        }
    }

    /// A child still running past the deadline is killed and reaped:
    /// `TimedOut`, and the stored exit status (available immediately,
    /// without blocking) shows the kill.
    #[test]
    fn wait_with_deadline_kills_and_reaps_a_child_past_the_deadline() {
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");
        // The deadline has already passed, so the first poll kills.
        let deadline = std::time::Instant::now() - std::time::Duration::from_secs(1);
        match super::wait_with_deadline(&mut child, deadline).expect("poll") {
            super::WaitOutcome::TimedOut => {}
            super::WaitOutcome::Exited(status) => panic!("sleep 60 must time out: {status}"),
        }
        assert_reaped_as_killed(&mut child);
    }

    /// The ceremony helper returns the exit status and the drained stderr
    /// together for a child that exits before the deadline — the caller
    /// never joins the reader itself.
    #[test]
    fn wait_with_deadline_or_timeout_returns_status_and_stderr_for_a_fast_child() {
        let mut child = std::process::Command::new("sh")
            .args(["-c", "echo hello-stderr >&2"])
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn sh");
        let stderr_reader = super::read_to_string(child.stderr.take().expect("stderr is piped"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let (status, stderr) = super::wait_with_deadline_or_timeout(
            &mut child,
            deadline,
            stderr_reader,
            "waiting for test child",
            |stderr| format!("timed out: {stderr}"),
        )
        .expect("a fast child exits");
        assert!(status.success());
        assert_eq!(stderr.trim(), "hello-stderr");
    }

    /// A child still running at the deadline is killed and reaped, and the
    /// timeout error is the call site's message built from the drained
    /// stderr — the reader is joined exactly once, inside the helper.
    #[test]
    fn wait_with_deadline_or_timeout_times_out_with_the_drained_stderr() {
        // `exec` keeps the sleep on the spawned pid: killing the child
        // kills the sleep, so the pipe closes at once (a forked
        // grandchild would inherit the pipe and hold the reader's join
        // for the whole sleep).
        let mut child = std::process::Command::new("sh")
            .args(["-c", "echo partial-progress >&2; exec sleep 60"])
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn sh");
        let stderr_reader = super::read_to_string(child.stderr.take().expect("stderr is piped"));
        // Long enough for the echo to land, short enough to keep the test
        // fast.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let err = super::wait_with_deadline_or_timeout(
            &mut child,
            deadline,
            stderr_reader,
            "waiting for test child",
            |stderr| format!("child timed out; stderr: {stderr}"),
        )
        .expect_err("a child past its deadline times out");
        assert_eq!(err, "child timed out; stderr: partial-progress\n");
        assert_reaped_as_killed(&mut child);
    }
}
