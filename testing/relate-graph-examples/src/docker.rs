//! The Neo4j EE reference container (ADR-0144): one `neo4j:2026.09-enterprise`
//! docker container per suite run, one same-named database per dataset.
//!
//! Dumps are loaded through one-off `neo4j-admin database load` containers
//! writing into a shared docker volume **before** the server container
//! boots: loading while the server runs leaves the database quarantined
//! (`quarantine_marker`), and a quarantined store never comes online. With
//! the server absent during load, the store mounts cleanly when
//! `CREATE DATABASE` registers it.
//!
//! Container and volume names are unique per run ([`RunNames`]): the shared
//! `relate-graph-examples-reference` base plus a per-process suffix
//! (pid, a process-local counter, and a time-based splitmix, so even two
//! pid-namespaced processes sharing one docker daemon cannot collide).
//! Two concurrent suite runs on one host (a local run beside a sibling
//! worktree's run) therefore never share a reference. Cleanup flows through
//! one armed [`CleanupGuard`]: armed right after the pre-clean, its `Drop`
//! removes exactly this run's container and volume on every early return;
//! the success path disarms it into [`DatasetTest`], whose `Drop` owns the
//! same removal. Cleanup therefore only ever removes THIS run's objects —
//! never a sibling's live reference mid-pull. The nightly workflow's
//! diagnostics step finds the container by the shared name prefix
//! (`docker ps -aq --filter name=relate-graph-examples-reference`). A
//! run killed before its `Drop` (SIGKILL, runner timeout) leaves its
//! uniquely-named container and volume behind for `docker system prune`;
//! that leak is the price of never removing a sibling's live reference.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const IMAGE: &str = "neo4j:2026.09-enterprise";
/// Name prefix of the per-run reference container. The nightly workflow's
/// diagnostics step filters on this prefix, so the per-run suffix is
/// appended after it.
const CONTAINER_BASE: &str = "relate-graph-examples-reference";
/// Name prefix of the per-run reference data volume.
const VOLUME_BASE: &str = "relate-graph-examples-reference-data";
const HTTP_PORT: u16 = 7474;
const BOLT_PORT: u16 = 7687;

/// This run's docker object names: the shared base plus a per-process
/// suffix (pid, a process-local counter, and a time-based splitmix), so two
/// concurrent suite runs — or two instances in one process — never name
/// the same container or volume, and a run's cleanup can only ever remove
/// its own objects.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RunNames {
    container: String,
    volume: String,
}

impl RunNames {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(counter);
        let suffix = run_suffix(std::process::id(), nanos, counter);
        Self {
            container: format!("{CONTAINER_BASE}-{suffix}"),
            volume: format!("{VOLUME_BASE}-{suffix}"),
        }
    }
}

/// The per-run name suffix. The pid alone is not enough: two pid-namespaced
/// processes sharing one docker daemon can both be pid 1, and both start at
/// counter 0. The wall clock keeps those apart, and the counter keeps two
/// calls within one process apart even when the clock does not move.
/// splitmix64 (public-domain mixing, no dependency) spreads the mixed bits
/// over the whole word, and the result stays inside docker's name charset
/// (`[A-Za-z0-9_.-]`).
fn run_suffix(pid: u32, nanos: u64, counter: u64) -> String {
    let mut x = (u64::from(pid) << 32) ^ nanos ^ counter;
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    format!("{pid}-{x:016x}")
}

/// Removes this run's container and volume when dropped while armed. Armed
/// in [`start_with_dumps`] right after the pre-clean, so every early return
/// from there to the successful boot leaves no container and no volume
/// behind; the success path disarms it into [`DatasetTest`], which owns
/// cleanup from then on. Dropping while disarmed removes nothing.
struct CleanupGuard<F: Fn(&RunNames) = fn(&RunNames)> {
    names: RunNames,
    remove: F,
    armed: bool,
}

impl CleanupGuard<fn(&RunNames)> {
    fn new(names: RunNames) -> Self {
        Self::with_remove(names, docker_cleanup)
    }
}

impl<F: Fn(&RunNames)> CleanupGuard<F> {
    /// A guard whose removals go through `remove` instead of docker — the
    /// seam that makes the cleanup contract testable without a docker
    /// daemon.
    fn with_remove(names: RunNames, remove: F) -> Self {
        Self {
            names,
            remove,
            armed: true,
        }
    }

    /// This run's names, for the docker commands that still have to run
    /// before the boot succeeds.
    fn names(&self) -> &RunNames {
        &self.names
    }

    /// Disarm and hand the names back: [`DatasetTest`] owns cleanup from
    /// here on.
    fn disarm(mut self) -> RunNames {
        self.armed = false;
        self.names.clone()
    }
}

impl<F: Fn(&RunNames)> Drop for CleanupGuard<F> {
    fn drop(&mut self) {
        if self.armed {
            (self.remove)(&self.names);
        }
    }
}

/// Remove this run's container and volume, best effort — a `Drop` has no
/// way to report, and `rm -f` on an object that was never created is a
/// harmless no-op.
fn docker_cleanup(names: &RunNames) {
    for command in cleanup_commands(names) {
        let args: Vec<&str> = command.iter().map(String::as_str).collect();
        let _ = docker(&args);
    }
}

/// The exact docker commands that remove this run's container and volume.
/// Pure, so the guard's removal contract is testable without docker.
fn cleanup_commands(names: &RunNames) -> [Vec<String>; 2] {
    [
        vec!["rm".into(), "-f".into(), names.container.clone()],
        vec![
            "volume".into(),
            "rm".into(),
            "-f".into(),
            names.volume.clone(),
        ],
    ]
}

/// Is docker usable on this host (CLI present and responsive)?
pub fn docker_available() -> bool {
    Command::new("docker")
        .arg("info")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The running reference container. Dropping it removes THIS run's
/// container and data volume — the suite never leaks a reference behind,
/// and never removes a sibling run's.
pub struct DatasetTest {
    http_port: u16,
    names: RunNames,
}

/// Load every dump into this run's volume through one-off admin
/// containers, then boot the server on that volume.
pub fn start_with_dumps(dumps: &[(String, std::path::PathBuf)]) -> Result<DatasetTest, String> {
    let names = RunNames::new();
    // The time-based suffix makes a name collision with an earlier
    // incarnation unlikely; the pre-clean stays as a cheap safety net for
    // an object that somehow already holds these exact names. A sibling
    // run's objects have different names and are never touched.
    docker_cleanup(&names);
    // From here to the successful boot, every early return must leave no
    // container and no volume behind; the armed guard's `Drop` is the one
    // cleanup path for all of them.
    let guard = CleanupGuard::new(names);
    docker(&["volume", "create", &guard.names().volume])?;
    for (database, dump) in dumps {
        load_via_admin_container(guard.names(), database, dump)?;
    }
    let auth = format!(
        "NEO4J_AUTH={}/{}",
        crate::reference::REFERENCE_USER,
        crate::reference::REFERENCE_PASSWORD
    );
    let out = docker(&[
        "run",
        "-d",
        "--name",
        &guard.names().container,
        "-v",
        &format!("{}:/data", guard.names().volume),
        "-e",
        "NEO4J_ACCEPT_LICENSE_AGREEMENT=yes",
        "-e",
        &auth,
        // The default quarter-of-VM heap OOMs the reference on the heavier
        // adoc queries; give it an explicit 4 GB.
        "-e",
        "NEO4J_server_memory_heap_max__size=4G",
        // No server-wide `db.transaction.timeout` is set, deliberately: the
        // pull runs one multi-minute transaction that such a timeout would
        // kill. A runaway adoc statement is instead terminated individually
        // after its timebox fires (reference::terminate_statement_transactions).
        // Loopback-only publish: the reference runs with repo-public
        // credentials, so binding 0.0.0.0 would leave it admin-reachable
        // from the network for the whole run.
        "-p",
        &format!("127.0.0.1::{HTTP_PORT}"),
        "-p",
        &format!("127.0.0.1::{BOLT_PORT}"),
        IMAGE,
    ])?;
    let container_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if container_id.is_empty() {
        return Err("docker run printed no container id".into());
    }
    let http_port =
        mapped_port(&container_id, HTTP_PORT).and_then(|p| wait_http_ready(&p).map(|()| p))?;
    Ok(DatasetTest {
        http_port,
        names: guard.disarm(),
    })
}

/// One-off admin container: `neo4j-admin database load --from-stdin
/// <database>` with the dump file as stdin (the shell equivalent of
/// `neo4j-admin database load - < dump`). No server is running, so the store
/// is written without a quarantine marker. stderr is drained while the
/// process runs (never left to fill the pipe) and a hard timeout guards a
/// wedged load.
fn load_via_admin_container(names: &RunNames, database: &str, dump: &Path) -> Result<(), String> {
    let dump_file =
        std::fs::File::open(dump).map_err(|e| format!("opening {}: {e}", dump.display()))?;
    let mut child = Command::new("docker")
        .args([
            "run",
            "-i",
            "--rm",
            "-v",
            &format!("{}:/data", names.volume),
            "-e",
            "NEO4J_ACCEPT_LICENSE_AGREEMENT=yes",
            IMAGE,
            "neo4j-admin",
            "database",
            "load",
            "--from-stdin",
            database,
        ])
        .stdin(std::process::Stdio::from(dump_file))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning admin load for {database}: {e}"))?;
    // Drain stderr on a thread so the pipe can never fill and deadlock the
    // child.
    let err_handle = crate::child::read_to_string(child.stderr.take().expect("stderr is piped"));
    let deadline = Instant::now() + Duration::from_secs(300);
    let (status, stderr_text) = crate::child::wait_with_deadline_or_timeout(
        &mut child,
        deadline,
        err_handle,
        &format!("waiting for admin load {database}"),
        |stderr| {
            format!("admin load {database} timed out after 300 s; neo4j-admin stderr: {stderr}")
        },
    )?;
    if !status.success() {
        return Err(format!(
            "admin load {database} failed ({status}):\n{stderr_text}"
        ));
    }
    Ok(())
}

impl DatasetTest {
    /// Host-side HTTP port of the reference.
    pub fn http_port(&self) -> u16 {
        self.http_port
    }

    /// Point URI for pull env-files and Query API v2 credentials.
    pub fn uri(&self) -> String {
        format!("http://localhost:{}", self.http_port)
    }

    /// `relate pull` `--from` env-file contents for one database.
    pub fn pull_env_file(&self, database: &str) -> String {
        format!(
            "NEO4J_URI={}\nNEO4J_USERNAME={}\nNEO4J_PASSWORD={}\nNEO4J_DATABASE={database}\n",
            self.uri(),
            crate::reference::REFERENCE_USER,
            crate::reference::REFERENCE_PASSWORD,
        )
    }

    /// Register one loaded store as a database (`CREATE DATABASE IF NOT
    /// EXISTS` against `system`) and wait for it to come online. The volume
    /// was loaded before the server booted, so there is no quarantine
    /// marker to clear.
    pub fn ensure_database(&self, database: &str) -> Result<(), String> {
        let creds = crate::reference::database_credentials_uri(&self.uri(), "system");
        let mut params = serde_json::Map::new();
        params.insert(
            "name".into(),
            serde_json::Value::String(database.to_string()),
        );
        let _ =
            relate_load::run_query(&creds, "CREATE DATABASE $name IF NOT EXISTS", &params, true)
                .map_err(|e| format!("creating database {database}: {e}"))?;
        // The database comes online asynchronously; poll a trivial query
        // until it answers (or time out).
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            match relate_load::run_query(
                &crate::reference::database_credentials_uri(&self.uri(), database),
                "RETURN 1",
                &Default::default(),
                true,
            ) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    if Instant::now() > deadline {
                        return Err(format!("database {database} not online: {e}"));
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    }
}

/// Capture every reference container's logs into `report_dir` as
/// `reference-<id>.log`, best effort (ADR-0204). On an infrastructure
/// failure the suite panics and the unwind removes the container via the
/// [`DatasetTest`]/[`CleanupGuard`] `Drop`s, so by the time the nightly
/// workflow's own capture step runs there is nothing left to inspect.
/// Calling this BEFORE the panic puts the logs next to report.txt, where
/// the workflow's existing `reference-*.log` upload glob already picks
/// them up — no workflow change needed.
///
/// No credential stripping is needed here: `docker logs` carries only the
/// process's stdout/stderr, never the container's `Config.Env` (which is
/// why the workflow strips env only from `docker inspect` output).
///
/// Never fails: every error (docker gone, container racing its own
/// removal, unwritable report dir) degrades to a stderr note.
pub fn capture_reference_logs(report_dir: &Path) {
    // The canonical helper: one place owns spawn-error and non-zero-exit
    // rendering; the never-fails policy here degrades any error to a
    // stderr note and returns.
    let ids = match docker(&["ps", "-aq", "--filter", &format!("name={CONTAINER_BASE}")]) {
        Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned(),
        Err(e) => {
            eprintln!("reference log capture: {e}");
            return;
        }
    };
    for id in ids.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let path = report_dir.join(format!("reference-{id}.log"));
        let result = Command::new("docker")
            .args(["logs", id])
            .output()
            .and_then(|out| {
                // stdout first, then stderr (the server's log stream) — one
                // file per container, same as the workflow's `docker logs 2>&1`.
                let mut combined = out.stdout;
                combined.extend_from_slice(&out.stderr);
                std::fs::write(&path, combined)
            });
        match result {
            Ok(()) => eprintln!("reference log capture: {} -> {}", id, path.display()),
            Err(e) => eprintln!("reference log capture: {id}: {e}"),
        }
    }
}

impl Drop for DatasetTest {
    fn drop(&mut self) {
        // Only THIS run's objects: a sibling suite run's reference has a
        // different suffix and must survive our cleanup (and vice versa).
        docker_cleanup(&self.names);
    }
}

fn docker(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| format!("docker {}: {e}", args.join(" ")))
        .and_then(|o| {
            if o.status.success() {
                Ok(o)
            } else {
                Err(format!(
                    "docker {} failed ({}): {}",
                    args.join(" "),
                    o.status,
                    String::from_utf8_lossy(&o.stderr)
                ))
            }
        })
}

/// Host port docker mapped for `container_port`, from `docker port`.
fn mapped_port(container_id: &str, container_port: u16) -> Result<u16, String> {
    let out = docker(&["port", container_id])?;
    let text = String::from_utf8_lossy(&out.stdout);
    // Ports publish on loopback only, so lines look like
    // `7474/tcp -> 127.0.0.1:32768`.
    for line in text.lines() {
        let Some((src, dst)) = line.split_once(" -> ") else {
            continue;
        };
        if src.trim() != format!("{container_port}/tcp") {
            continue;
        }
        let Some(port) = dst
            .rfind(':')
            .and_then(|pos| dst[pos + 1..].parse::<u16>().ok())
        else {
            continue;
        };
        return Ok(port);
    }
    Err(format!(
        "no host mapping for port {container_port} in:\n{text}"
    ))
}

/// Poll the HTTP root until the server answers (the container is up) or
/// the timeout passes; a 2xx response means ready.
fn wait_http_ready(port: &u16) -> Result<(), String> {
    let url = format!("http://localhost:{port}/");
    let deadline = Instant::now() + Duration::from_secs(180);
    while Instant::now() < deadline {
        if ureq::get(&url)
            .config()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .call()
            .is_ok()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!("neo4j reference not ready on {url} within 180 s"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_names_are_unique_across_instances() {
        let a = RunNames::new();
        let b = RunNames::new();
        assert_ne!(a.container, b.container);
        assert_ne!(a.volume, b.volume);
        assert_ne!(a.container, a.volume);
    }

    #[test]
    fn run_names_suffix_mixes_in_the_clock() {
        // Two pid-namespaced processes sharing one docker daemon can both
        // be pid 1 and both start at counter 0; the clock separates them.
        assert_ne!(run_suffix(1, 1_000, 0), run_suffix(1, 1_001, 0));
        // Deterministic for identical inputs, and a different pid still
        // separates same-tick processes.
        assert_eq!(run_suffix(1, 1_000, 0), run_suffix(1, 1_000, 0));
        assert_ne!(run_suffix(1, 1_000, 0), run_suffix(2, 1_000, 0));
    }

    #[test]
    fn run_names_keep_the_shared_prefix() {
        // The nightly workflow's diagnostics step filters containers on the
        // shared prefix, so the per-run suffix must come after it.
        let names = RunNames::new();
        assert!(
            names
                .container
                .starts_with("relate-graph-examples-reference-"),
            "{}",
            names.container
        );
        assert!(
            names
                .volume
                .starts_with("relate-graph-examples-reference-data-"),
            "{}",
            names.volume
        );
        // Docker name charset: alphanumerics plus `_.-` — the
        // pid/counter/clock suffix stays inside it.
        assert!(
            names
                .container
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')),
            "{}",
            names.container
        );
    }

    #[test]
    fn cleanup_commands_remove_exactly_this_runs_objects() {
        let names = RunNames::new();
        assert_eq!(
            cleanup_commands(&names),
            [
                vec!["rm".to_string(), "-f".to_string(), names.container.clone()],
                vec![
                    "volume".to_string(),
                    "rm".to_string(),
                    "-f".to_string(),
                    names.volume.clone(),
                ],
            ]
        );
    }

    #[test]
    fn cleanup_guard_removes_this_run_when_armed_and_nothing_when_disarmed() {
        let removed: std::cell::RefCell<Vec<Vec<String>>> = std::cell::RefCell::new(Vec::new());

        let armed_names = RunNames::new();
        {
            let _guard = CleanupGuard::with_remove(armed_names.clone(), |names: &RunNames| {
                for command in cleanup_commands(names) {
                    removed.borrow_mut().push(command);
                }
            });
        }
        let expected = vec![
            vec![
                "rm".to_string(),
                "-f".to_string(),
                armed_names.container.clone(),
            ],
            vec![
                "volume".to_string(),
                "rm".to_string(),
                "-f".to_string(),
                armed_names.volume.clone(),
            ],
        ];
        assert_eq!(
            *removed.borrow(),
            expected,
            "an armed guard's Drop removes exactly this run's container and volume"
        );

        let disarmed_names = RunNames::new();
        let guard = CleanupGuard::with_remove(disarmed_names.clone(), |names: &RunNames| {
            for command in cleanup_commands(names) {
                removed.borrow_mut().push(command);
            }
        });
        assert_eq!(guard.disarm(), disarmed_names);
        assert_eq!(
            *removed.borrow(),
            expected,
            "a disarmed guard must not remove anything"
        );
    }

    #[test]
    fn pull_env_file_shape() {
        let reference = DatasetTest {
            http_port: 32768,
            names: RunNames::new(),
        };
        let env = reference.pull_env_file("movies");
        assert!(env.contains("NEO4J_URI=http://localhost:32768\n"));
        assert!(env.contains(&format!(
            "NEO4J_USERNAME={}\n",
            crate::reference::REFERENCE_USER
        )));
        assert!(env.contains(&format!(
            "NEO4J_PASSWORD={}\n",
            crate::reference::REFERENCE_PASSWORD
        )));
        assert!(env.contains("NEO4J_DATABASE=movies\n"));
    }
}
