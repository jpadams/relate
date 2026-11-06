//! Container entrypoint (ADR-0128, retargeted by ADR-0249) — the scratch
//! image has no shell, so the seed-then-serve logic is this tiny binary.
//! It drives `relate serve`: one process hosting Bolt (7687), the HTTP
//! Query API v2 (7474), and MCP (7475) on the one open, seeded store with
//! one shared credential.
//!
//! Config resolution, most to least canonical:
//! 1. **Container args pass through to `relate serve`** — the
//!    Dockerfile `CMD` defaults (`--host 0.0.0.0 --database /data/graph
//!    --bolt 7687 --http 7474 --mcp 7475`) are replaced by anything after
//!    the image name. The entrypoint reads the effective store and bind
//!    plan (for seeding and fleet ports) and passes the rest on. The
//!    ADR-0128 bolt-binary spellings are accepted as aliases: `--uri` ≡
//!    `--database`, `--bind HOST:PORT` ≡ `--host HOST --bolt PORT`, and
//!    `--allow-plaintext` is dropped with a note (serve is plaintext-only;
//!    a non-loopback `--host` warns loudly itself, ADR-0248).
//! 2. **`/config/seed.json`** (mounted config file; path override:
//!    `RELATE_CONFIG`) drives seeding when it exists — ordered steps,
//!    one file instead of a pile of envs:
//!    `{ "force": false, "steps": [
//!        {"query": "UNWIND range(1,100) AS i CREATE (:Person {id: i})"},
//!        {"load": "value=https://…", "query": "UNWIND $value.items AS q MERGE …"},
//!        {"pull": "/config/neo4j.env", "query": "MATCH (o)-[r]-() RETURN o, r"} ]}`
//! 3. **Env fallback** when no file exists: `RELATE_QUERY`,
//!    `RELATE_LOAD` + `RELATE_LOAD_QUERY`,
//!    `RELATE_PULL_FROM` + `RELATE_PULL_QUERY`,
//!    `RELATE_FORCE_SEED=1` (reseed; `:memory:` always seeds).
//!
//! Memory, durability, checkpointing and LOAD CSV allowlisting stay on the
//! engine's native env vars, passed through the container untouched.
//!
//! Layout: `args` (argv tokenization/redaction/resolution), `seed`
//! (seed.json + env fallback + step execution), `fleet` (INSTANCES=N
//! supervision); this root is the single-server path and orchestration.

mod args;
mod fleet;
mod seed;

// Unix-only leaves (`.exec()`, SIGTERM via the lib's signal-hook signal
// module) are gated the same way the server's main.rs gates them, so
// `cargo build/test -p relate-bolt` keeps working on Windows even
// though the image is Linux-only.
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use args::{
    database_arg, note_dropped_legacy_args, redacted_args, resolve_plan, serve_argv,
    warn_if_loopback,
};
use seed::{seed_policy, seed_unless_exists, uri_fs_path, DEFAULT_CONFIG};

pub(crate) const BIN_DIR: &str = "/bin";
const DEFAULT_URI: &str = "/data/graph";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let instances = instances_env();

    if instances == 1 {
        serve_single(&args);
    } else {
        fleet::serve_fleet(&args, instances);
    }
}

/// `INSTANCES` (default 1): 1..=65535, anything else is a loud error.
fn instances_env() -> usize {
    match std::env::var("INSTANCES") {
        Ok(v) => match v.parse::<usize>() {
            Ok(n) if (1..=65535).contains(&n) => n,
            _ => {
                eprintln!("entrypoint: invalid INSTANCES={v} (expected an integer 1..=65535)");
                std::process::exit(1);
            }
        },
        Err(_) => 1,
    }
}

/// The effective store URI from argv (`--database`, or the `--uri`
/// alias), defaulting to the image's `/data/graph`; a conflict between
/// the aliases is a loud startup error.
pub(crate) fn resolve_uri(args: &[String]) -> String {
    match database_arg(args) {
        Ok(Some(uri)) => uri.to_string(),
        Ok(None) => DEFAULT_URI.to_string(),
        Err(e) => {
            eprintln!("entrypoint: {e}");
            std::process::exit(1);
        }
    }
}

/// The common pre-serve work for one instance: its effective URI
/// (`uri_suffix` is Some(i) on the fleet path), with the URI's parent
/// directory created best-effort.
pub(crate) fn prepare_instance(args: &[String], uri_suffix: Option<usize>) -> String {
    let base = resolve_uri(args);
    let uri = match uri_suffix {
        Some(i) => seed::suffix_uri(&base, i),
        None => base,
    };
    // The volume mount may be an empty dir; make sure the URI's parent
    // exists (best effort — the engine errors clearly otherwise).
    if let Some(parent) = Path::new(uri_fs_path(&uri)).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    uri
}

fn serve_single(args: &[String]) {
    note_dropped_legacy_args(args);
    let uri = prepare_instance(args, None);

    let (config, force_seed) = seed_policy();

    // Startup banner: the effective configuration, so a misbehaving
    // container is diagnosable from `docker logs` alone.
    let config_source =
        std::env::var("RELATE_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let seed_plan = match &config {
        Some(c) => c.plan_summary(),
        None => "env fallback (RELATE_QUERY, RELATE_LOAD, RELATE_PULL_*)".to_string(),
    };
    eprintln!(
        "entrypoint: uri={uri} args={:?} config={config_source} seed={seed_plan}",
        redacted_args(args)
    );

    // The loopback warning checks the EFFECTIVE host: an omitted --host
    // resolves to the image default (all interfaces), so only an
    // explicitly loopback host warns.
    let plan = match resolve_plan(args) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("entrypoint: {e}");
            std::process::exit(1);
        }
    };
    warn_if_loopback(&plan.host);

    seed_unless_exists(&uri, &uri, &config, force_seed);

    exec_server(&serve_argv(args, &plan, &uri));
}

/// Replace this process with `relate serve` (unix `exec`): it becomes
/// PID 1 and owns signals and the container lifecycle. Off-unix there is
/// no `exec` and the image is Linux-only, so fail loudly instead of
/// serving a supervised child with no PID-1 semantics.
#[cfg(unix)]
fn exec_server(args: &[String]) -> ! {
    let error = Command::new(format!("{BIN_DIR}/relate")).args(args).exec();
    eprintln!("entrypoint: failed to exec relate serve: {error}");
    std::process::exit(1);
}

#[cfg(not(unix))]
fn exec_server(args: &[String]) -> ! {
    let _ = args;
    eprintln!("entrypoint: the container entrypoint is only supported on unix");
    std::process::exit(1);
}
