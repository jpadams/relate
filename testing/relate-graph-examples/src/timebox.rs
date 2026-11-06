//! Hard per-statement timeboxes (ADR-0144). A differential run must never
//! hang on one query: every adoc statement gets a budget on both engines,
//! and exceeding it is a recorded outcome naming the statement, not a hung
//! nightly. Pull pages get their own (larger) budget.

/// Per-statement query budget: `RELATE_GRAPH_EXAMPLES_QUERY_TIMEOUT_SECS`,
/// default 20 s. Applies to one statement on either engine. On the
/// reference side an exceeded budget is inconclusive (ADR-0176: no
/// trustworthy expected result, nothing to diff); on the relate side it
/// is a recorded diff (engine slowness is real signal) — both are reported,
/// neither fails the run.
pub fn query_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(crate::config::env_or(
        "RELATE_GRAPH_EXAMPLES_QUERY_TIMEOUT_SECS",
        20,
    ))
}

/// Per-pull-page budget: `RELATE_GRAPH_EXAMPLES_PULL_TIMEOUT_SECS`,
/// default 900 s. A page exceeding it kills the `relate pull` child and
/// fails the dataset — a hang on the pull path is a real signal, not an
/// infrastructure flake.
pub fn pull_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(crate::config::env_or(
        "RELATE_GRAPH_EXAMPLES_PULL_TIMEOUT_SECS",
        900,
    ))
}

/// The outcome of a [`run_timed`] call that did not produce a value: the
/// worker's own error stays typed (`Work`), while the timebox and
/// worker-panic cases are separate variants — no typed error is ever
/// smuggled through a stringly-typed channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError<E> {
    /// The worker finished within the budget and returned its own error.
    Work(E),
    /// The budget elapsed before the worker produced a result.
    TimedOut(String),
    /// The worker thread panicked (or otherwise dropped its result).
    Panicked(String),
}

impl<E: std::fmt::Display> std::fmt::Display for RunError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Work(e) => write!(f, "{e}"),
            RunError::TimedOut(why) | RunError::Panicked(why) => f.write_str(why),
        }
    }
}

impl<E> std::error::Error for RunError<E> where E: std::error::Error + 'static {}

/// Run `f` under the query budget. `f`'s result travels back typed; if the
/// budget elapses first, the wait is abandoned and [`RunError::TimedOut`] is
/// returned instead. The worker thread is detached: it may finish later,
/// but the caller never waits past the budget. Used where the work cannot
/// be cancelled from the inside (the reference's synchronous HTTP call).
pub fn run_timed<T, E>(
    budget: std::time::Duration,
    what: &str,
    f: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, RunError<E>>
where
    T: Send + 'static,
    E: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<Result<T, E>>();
    std::thread::spawn(move || {
        // A send failure means the caller already timed out; drop the result.
        // A panic drops `tx`, which surfaces below as a worker-panicked
        // error, not as a timeout.
        let _ = tx.send(f());
    });
    match rx.recv_timeout(budget) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(RunError::Work(e)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(RunError::TimedOut(format!(
            "{what} exceeded the {}s timebox",
            budget.as_secs()
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(RunError::Panicked(format!(
            "{what} worker panicked; no result was produced"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_work_returns_its_result() {
        let r = run_timed(std::time::Duration::from_secs(5), "fast", || {
            Ok::<_, String>(42)
        });
        assert_eq!(r.unwrap(), 42);
    }

    /// The worker's own error comes back typed in `RunError::Work` — never
    /// flattened into the timebox/panic string channel.
    #[test]
    fn worker_error_stays_typed_in_the_work_variant() {
        #[derive(Debug, PartialEq, Eq)]
        struct WorkerError(u32);
        impl std::fmt::Display for WorkerError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "worker error {}", self.0)
            }
        }
        impl std::error::Error for WorkerError {}
        let r: Result<(), RunError<WorkerError>> =
            run_timed(std::time::Duration::from_secs(5), "typed", || {
                Err(WorkerError(7))
            });
        match r {
            Err(RunError::Work(e)) => assert_eq!(e, WorkerError(7)),
            other => panic!("worker error must be RunError::Work: {other:?}"),
        }
    }

    #[test]
    fn slow_work_returns_the_timeout_error() {
        let r: Result<(), RunError<String>> =
            run_timed(std::time::Duration::from_millis(100), "slow", || {
                std::thread::sleep(std::time::Duration::from_secs(30));
                Ok(())
            });
        match r {
            Err(RunError::TimedOut(why)) => assert!(why.contains("exceeded the 0s timebox")),
            other => panic!("slow work must time out: {other:?}"),
        }
    }

    #[test]
    fn a_panicking_worker_is_not_a_timeout() {
        let r: Result<(), RunError<String>> =
            run_timed(std::time::Duration::from_secs(5), "boom", || {
                panic!("worker bug")
            });
        match r {
            Err(RunError::Panicked(why)) => assert!(why.contains("worker panicked")),
            other => panic!("a panicking worker must be RunError::Panicked: {other:?}"),
        }
    }
}
