//! End-to-end `relate pull` invocation (ADR-0144): the suite spawns the
//! real binary (`RELATE_BIN`) with an env-file for the reference container
//! and the whole-database shape queries — the same product surface a user or
//! agent runs.
//!
//! The built binary is not reachable from a `testing/` crate through Cargo
//! (only `relate-cli`'s own tests see `CARGO_BIN_EXE_relate`), so
//! the suite takes the path from `RELATE_BIN` — resolved by the shared
//! `relate-test-support` helpers — and fails with build instructions
//! when it is unset.
use relate_test_support::{harness_child_env, relate_bin, relate_bin_missing_message};
use std::path::Path;
use std::process::{Command, Stdio};

/// The two shape statements of a whole-database pull (ADR-0158): nodes
/// first, then rels that carry their endpoint nodes.
pub const WHOLE_DATABASE_QUERY: &str = "MATCH (n) RETURN n AS e; \
     MATCH (a)-[r]->(b) RETURN a AS x, r AS e, b AS y";

/// The whole-database pull: one `relate pull` invocation carrying both
/// shape statements ([`WHOLE_DATABASE_QUERY`]). One process: the response
/// streams, rows materialize in batches, one checkpoint at the end. No
/// paging, no per-page process.
pub fn pull_whole_database(
    database: &str,
    destination: &Path,
    env_file: &Path,
) -> Result<String, String> {
    pull_once(database, destination, env_file, WHOLE_DATABASE_QUERY)
}

/// The `relate pull` child command for one run: the argv of
/// [`pull_once`] plus the shared harness env policy
/// (`relate_test_support::harness_child_env`: the ADR-0079 §6
/// telemetry opt-out and the ambient `RELATE_*` knob scrub) — the
/// differential suite is a nightly CI harness spawn, not product usage,
/// so it must never emit events or inherit a developer shell's knobs.
/// Split out from [`pull_once`] so a unit test can pin the spawn env on
/// the constructed command, without the reference container.
fn pull_command(bin: &Path, destination: &Path, env_file: &Path, query: &str) -> Command {
    let mut command = Command::new(bin);
    command
        .arg("pull")
        .arg("--from")
        .arg(env_file)
        .arg("--query")
        .arg(query)
        // ADR-0200: the store path is `--database`, not a trailing
        // positional (the positional form was removed with the `query`
        // verb migration, PR #234).
        .arg("--database")
        .arg(destination)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    harness_child_env(&mut command);
    command
}

/// One pull run under the pull timebox (ADR-0144): a pull that exceeds the
/// budget is killed, so one slow pull fails the dataset instead of hanging
/// the nightly. Both pipes drain on reader threads, so a chatty child can
/// never block on write. Returns the CLI's log.
fn pull_once(
    database: &str,
    destination: &Path,
    env_file: &Path,
    query: &str,
) -> Result<String, String> {
    let bin = relate_bin().ok_or_else(relate_bin_missing_message)?;
    let mut child = pull_command(&bin, destination, env_file, query)
        .spawn()
        .map_err(|e| format!("spawning {}: {e}", bin.display()))?;
    let stdout_reader = crate::child::read_to_string(child.stdout.take().expect("stdout is piped"));
    let stderr_reader = crate::child::read_to_string(child.stderr.take().expect("stderr is piped"));
    let deadline = std::time::Instant::now() + crate::timebox::pull_timeout();
    let (status, stderr) = crate::child::wait_with_deadline_or_timeout(
        &mut child,
        deadline,
        stderr_reader,
        "waiting for pull",
        |stderr| {
            format!(
                "relate pull {database} exceeded the {}s timebox; stderr: {stderr}",
                crate::timebox::pull_timeout().as_secs()
            )
        },
    )?;
    let stdout = stdout_reader
        .join()
        .expect("the stdout reader thread finishes");
    if !status.success() {
        return Err(format!(
            "relate pull {database} failed ({status}):\n{stderr}"
        ));
    }
    Ok(stdout)
}

/// The transient retry signature (ADR-0204), anchored on the one stable
/// substring of relate-load's mid-statement read error
/// (`mid_statement_read_error` in `surfaces/relate-load/src/response.rs`):
/// `reading response body from <url> failed mid-statement: <io error>`.
/// The io-error tail varies by platform (`Peer disconnected`,
/// `Connection reset by peer (os error 54)`, …), so only the
/// `failed mid-statement` shape the loader itself emits is matched — a
/// bare disconnect mention from any other source does not retry. Auth
/// errors, syntax errors, and the pull timebox kill never carry the
/// substring, so they are never retried.
pub fn is_transient_failure(error: &str) -> bool {
    error.contains("failed mid-statement")
}

#[cfg(test)]
mod tests {
    /// ADR-0204: the retry signature matches only the mid-statement
    /// stream-disconnect class relate-load produces (the real text from
    /// nightly run 38108996036 first), never auth, syntax, or timebox-kill
    /// errors, and never a bare disconnect mention without the loader's
    /// `failed mid-statement` shape.
    #[test]
    fn transient_signature_matches_only_mid_statement_disconnects() {
        // The exact failure shape from the 2026-10-11 nightly (run
        // 38108996036), wrapped by pull_once's status prefix.
        let nightly = "relate pull fraud-detection failed (exit status: 1):\n\
             Error: reading response body from \
             http://localhost:32768/db/fraud-detection/query/v2 failed mid-statement: \
             Peer disconnected; the server closed the response stream";
        assert!(super::is_transient_failure(nightly));
        // Same path, different io error text (e.g. ECONNRESET on macOS).
        assert!(super::is_transient_failure(
            "reading response body from http://localhost:7474/db/movies/query/v2 \
             failed mid-statement: Connection reset by peer (os error 54)"
        ));
        // A bare disconnect mention without the loader's mid-statement
        // shape: not the transient class, never retried.
        assert!(!super::is_transient_failure("Peer disconnected"));
        // Auth and syntax failures: never retried.
        assert!(!super::is_transient_failure(
            "relate pull movies failed (exit status: 1):\n\
             Error: authentication failed for user neo4j"
        ));
        assert!(!super::is_transient_failure(
            "Neo.ClientError.Statement.SyntaxError: Invalid input"
        ));
        // The pull timebox kill: never retried.
        assert!(!super::is_transient_failure(
            "relate pull movies exceeded the 900s timebox; stderr: ..."
        ));
    }

    #[test]
    fn pull_child_is_telemetry_silent() {
        use std::ffi::OsStr;
        use std::path::Path;

        // The differential suite's `relate pull` child is a nightly CI
        // harness spawn, not product usage: the command must carry
        // DO_NOT_TRACK=1 (ADR-0079 §6) so a CI pull never emits telemetry.
        // Pinned on the constructed command's env (get_envs) — no
        // RELATE_BIN and no reference container needed.
        let command = super::pull_command(
            Path::new("relate"),
            Path::new("graph.rl"),
            Path::new("neo4j.env"),
            super::WHOLE_DATABASE_QUERY,
        );
        let do_not_track = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("DO_NOT_TRACK"))
            .and_then(|(_, value)| value);
        assert_eq!(
            do_not_track,
            Some(OsStr::new("1")),
            "the pull child must run with DO_NOT_TRACK=1"
        );
    }

    #[test]
    fn shape_queries_return_only_graph_elements() {
        // The same contract pull enforces (assert_graph_shape, ADR-0080):
        // every RETURN item is a bare variable, so each statement is a
        // legal shape.
        let statements = relate_load::split_statements(super::WHOLE_DATABASE_QUERY).unwrap();
        assert_eq!(statements.len(), 2, "{statements:?}");
        for q in &statements {
            relate_load::assert_graph_shape(q)
                .unwrap_or_else(|e| panic!("{q} must be graph-shaped: {e}"));
        }
    }
}
