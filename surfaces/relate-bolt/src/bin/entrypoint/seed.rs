//! Seed configuration and execution: `/config/seed.json` parsing, the env
//! fallback, the seed/skip decision, and running the steps through the
//! `relate` CLI before any server starts.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::BIN_DIR;

pub(crate) const DEFAULT_CONFIG: &str = "/config/seed.json";

/// `/config/seed.json` — ordered seed steps; each step is exactly one of
/// `query`, `load` + `query`, or `pull` + `query`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SeedConfig {
    #[serde(default)]
    pub(crate) force: bool,
    #[serde(default)]
    steps: Vec<SeedStep>,
}

impl SeedConfig {
    /// The seed plan for the startup banner: step count and force flag.
    pub(crate) fn plan_summary(&self) -> String {
        format!(
            "{} step(s){}",
            self.steps.len(),
            if self.force { ", force" } else { "" }
        )
    }
}

/// One seed step: exactly one of the three shapes, in the type system.
/// (An untagged serde enum would be the obvious encoding but cannot keep
/// the error contract: untagged drops `deny_unknown_fields` support
/// (serde#1547) and variant errors name no fields. So: a raw struct with
/// deny_unknown_fields, promoted at parse time by `try_from` — the
/// conflict error names the step and the unknown-field error names the
/// field.)
#[derive(Debug)]
enum SeedStep {
    /// A Cypher statement to execute (synthetic data, MERGE seeds…).
    Query { query: String },
    /// `NAME=SOURCE` for the `--load` flag (URL or file; needs a build
    /// with the `load` feature) — `query` then consumes `$NAME`.
    Load { load: String, query: String },
    /// A pull env file (`NEO4J_URI=…` Query API origin) for
    /// `relate pull --from` — `query` is the pull shape query.
    Pull { pull: String, query: String },
}

/// The wire shape: every field optional except `query`; the promotion to
/// [`SeedStep`] rejects the `load`+`pull` conflict.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStep {
    query: String,
    #[serde(default)]
    load: Option<String>,
    #[serde(default)]
    pull: Option<String>,
}

impl TryFrom<RawStep> for SeedStep {
    type Error = String;
    fn try_from(raw: RawStep) -> Result<Self, String> {
        match (raw.load, raw.pull) {
            (None, None) => Ok(SeedStep::Query { query: raw.query }),
            (Some(load), None) => Ok(SeedStep::Load {
                load,
                query: raw.query,
            }),
            (None, Some(pull)) => Ok(SeedStep::Pull {
                pull,
                query: raw.query,
            }),
            (Some(_), Some(_)) => Err(format!(
                "seed step sets both load and pull; a step is exactly one of query, load+query, or pull+query — step: {}",
                raw.query
            )),
        }
    }
}

impl<'de> Deserialize<'de> for SeedStep {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RawStep::deserialize(d)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// The seed config: an explicitly-set `RELATE_CONFIG` path (an
/// unreadable or invalid one is an error, not a silent env fallback; a
/// missing one falls back to env seeding), else the default path when a
/// regular file is present, else None (env fallback).
fn load_config() -> Option<SeedConfig> {
    match std::env::var("RELATE_CONFIG") {
        Ok(path) => read_config(&path),
        Err(_) => std::fs::metadata(DEFAULT_CONFIG)
            .ok()
            .filter(|m| m.is_file())
            .and_then(|_| read_config(DEFAULT_CONFIG)),
    }
}

/// Read and parse a seed config: None on a missing file (the caller falls
/// back to env seeding); anything else wrong (unreadable, invalid JSON,
/// unknown fields) fails loudly with the path and the reason.
fn read_config(path: &str) -> Option<SeedConfig> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("entrypoint: seed config {path} not found, using env fallback");
            return None;
        }
        Err(e) => {
            eprintln!("entrypoint: cannot read seed config {path}: {e}");
            std::process::exit(1);
        }
    };
    Some(serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("entrypoint: invalid {path}: {e}");
        eprintln!("entrypoint: expected {{\"force\": bool, \"steps\": [{{\"query\": \"…\" | \"load\": \"NAME=SOURCE\", \"query\": \"…\" | \"pull\": \"env file\", \"query\": \"…\"}}]}}");
        std::process::exit(1);
    }))
}

/// `RELATE_FORCE_SEED` must be 1 or true/yes — anything else is an
/// error, never a silent no-op.
fn force_seed_env() -> bool {
    match std::env::var("RELATE_FORCE_SEED") {
        Ok(v) => force_seed_value(&v).unwrap_or_else(|| {
            eprintln!("entrypoint: invalid RELATE_FORCE_SEED={v} (expected 1, true, or false)");
            std::process::exit(1);
        }),
        Err(_) => false,
    }
}

/// The real force-seed classifier: Some(true/false) for recognized values,
/// None for anything else. A value, not a boolean, so tests pin exactly
/// what production reads.
fn force_seed_value(v: &str) -> Option<bool> {
    match v {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// The URI as a filesystem path for the entrypoint's own checks
/// (existence, parent-dir creation): the engine accepts `file://` URIs
/// (the server's token path strips the same prefix), but `Path::new` on
/// the raw string would never match the on-disk database — and
/// `create_dir_all` would grow a literal `./file:` tree.
pub(crate) fn uri_fs_path(uri: &str) -> &str {
    uri.strip_prefix("file://").unwrap_or(uri)
}

/// Per-instance persistent URI: `graph` → `graph-{i}`, `graph.rl` →
/// `graph-{i}.rl`; a legacy `.cl` bundle keeps its own suffix (ADR-0517 §5),
/// and `:memory:` stays (each process gets its own).
pub(crate) fn suffix_uri(uri: &str, i: usize) -> String {
    if uri == ":memory:" {
        return uri.to_string();
    }
    if relate_storage::is_bundle_path(std::path::Path::new(uri)) {
        if let Some((stem, extension)) = uri.rsplit_once('.') {
            return format!("{stem}-{i}.{extension}");
        }
    }
    format!("{uri}-{i}")
}

/// Seed when the URI names no existing database (always for `:memory:`,
/// where nothing persists), or when reseeding is forced.
fn should_seed(uri: &str, force_seed: bool) -> bool {
    if force_seed {
        return true;
    }
    if uri == ":memory:" {
        return true;
    }
    !Path::new(uri_fs_path(uri)).exists()
}

/// The seed policy for one run: the parsed seed config (None = env
/// fallback) and the combined force flag (env var or seed.json "force").
/// One source for both serve paths — the two copies drifted once already
/// (the skip message disagreed on the accepted FORCE_SEED values).
pub(crate) fn seed_policy() -> (Option<SeedConfig>, bool) {
    let config = load_config();
    let force = force_seed_env() || config.as_ref().is_some_and(|c| c.force);
    (config, force)
}

/// Seed `uri` unless the database already exists (`:memory:` and forced
/// reseeds always run). `subject` names the database in the skip log
/// (plain URI for one server, "instance N (uri)" in a fleet).
pub(crate) fn seed_unless_exists(
    uri: &str,
    subject: &str,
    config: &Option<SeedConfig>,
    force: bool,
) {
    if should_seed(uri, force) {
        seed_instance(uri, config);
    } else {
        eprintln!(
            "entrypoint: {subject} exists, skipping seed (force: RELATE_FORCE_SEED=1|true|yes or seed.json \"force\": true)"
        );
    }
}

/// Run the seed steps from `seed.json`, in file order. The plan is
/// already valid by construction (the load+pull conflict fails at parse
/// time — before any step runs, so a bad plan never leaves a half-seeded
/// database that would skip seeding on the next boot), and the once-per
/// process `--load` probe also runs up front; a failing step's exit code
/// propagates and never starts the server half-seeded.
fn run_steps(uri: &str, steps: &[SeedStep]) {
    if steps.iter().any(|s| matches!(s, SeedStep::Load { .. })) {
        require_load_feature();
    }
    for step in steps {
        match step {
            SeedStep::Pull { pull, query } => {
                eprintln!("entrypoint: pulling {query}");
                run(&[
                    "relate".to_string(),
                    "pull".into(),
                    "--from".into(),
                    pull.clone(),
                    "--query".into(),
                    query.clone(),
                    // ADR-0200: the store path is `--database`, not a
                    // trailing positional.
                    "--database".into(),
                    uri.into(),
                ]);
            }
            SeedStep::Load { load, query } => {
                // The --load probe already ran up front (before any step).
                // Log only the parameter NAME: a load source can carry
                // credentials in its URL, and container logs outlive the run.
                eprintln!(
                    "entrypoint: loading param ${}",
                    load.split('=').next().unwrap_or("?")
                );
                run(&[
                    "relate".to_string(),
                    "--database".into(),
                    uri.into(),
                    "--load".into(),
                    load.clone(),
                    "-c".into(),
                    query.clone(),
                ]);
            }
            SeedStep::Query { query } => {
                eprintln!("entrypoint: running step query");
                run(&[
                    "relate".to_string(),
                    "--database".into(),
                    uri.into(),
                    "-c".into(),
                    query.clone(),
                ]);
            }
        }
    }
}

/// Seed one instance's URI: seed.json steps when a config exists, else the
/// env fallback.
fn seed_instance(uri: &str, config: &Option<SeedConfig>) {
    match config {
        Some(seed) => run_steps(uri, &seed.steps),
        None => seed_from_env(uri),
    }
}

/// Env fallback when no `seed.json` exists: lower the env vars into the
/// same `SeedStep` model the config file uses, so one execution path runs
/// both.
fn seed_from_env(uri: &str) {
    let mut steps = Vec::new();
    if let Ok(query) = std::env::var("RELATE_QUERY") {
        steps.push(SeedStep::Query { query });
    }
    if let Ok(load) = std::env::var("RELATE_LOAD") {
        let query = require_env("RELATE_LOAD_QUERY", "RELATE_LOAD");
        steps.push(SeedStep::Load { load, query });
    }
    if let Ok(pull_from) = std::env::var("RELATE_PULL_FROM") {
        let query = require_env("RELATE_PULL_QUERY", "RELATE_PULL_FROM");
        steps.push(SeedStep::Pull {
            pull: pull_from,
            query,
        });
    }
    run_steps(uri, &steps);
}

/// The CLI needs the `load` feature (its `--load` flag is on an unmerged
/// PR) for `--load`; without it the child would only say
/// "unknown flag: --load" with no hint which env or seed step caused it.
/// The answer is fixed for a given image, so probe once per process, not
/// once per load step. When the flag lands and the Dockerfile builds with
/// it, this guard becomes unreachable and can go.
static LOAD_SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn require_load_feature() {
    let supported = *LOAD_SUPPORTED.get_or_init(|| {
        Command::new(format!("{BIN_DIR}/relate"))
            .arg("--help")
            .output()
            .map(|o| help_offers_load_flag(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or(false)
    });
    if !supported {
        eprintln!("entrypoint: a load step is configured but this image's relate has no --load flag (build without the 'load' cargo feature); remove the load step or rebuild the image");
        std::process::exit(1);
    }
}

/// True when `--help` lists a `--load` flag. A line match, not a substring:
/// `--load-csv-dir` also contains `--load`, and without the boundary the
/// guard passes on builds that have no `--load` at all — the exact failure
/// it exists to explain.
fn help_offers_load_flag(help: &str) -> bool {
    help.lines().any(|l| {
        let l = l.trim_start();
        l == "--load" || l.starts_with("--load ") || l.starts_with("--load=")
    })
}

fn require_env(name: &str, needs: &str) -> String {
    match std::env::var(name) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("entrypoint: {needs} is set but {name} is not");
            std::process::exit(1);
        }
    }
}

fn run(argv: &[String]) {
    let program = format!("{BIN_DIR}/{}", argv[0]);
    let status = Command::new(program)
        .args(&argv[1..])
        .status()
        .unwrap_or_else(|e| {
            eprintln!("entrypoint: failed to run {}: {e}", argv[0]);
            std::process::exit(1);
        });
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_decision_by_uri_and_force() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("graph.rl");
        std::fs::write(&existing, b"x").unwrap();
        let existing = existing.to_str().unwrap();
        let missing = dir.path().join("nope.rl").to_str().unwrap().to_string();

        assert!(should_seed(":memory:", false));
        assert!(should_seed(&missing, false));
        assert!(!should_seed(existing, false));
        assert!(should_seed(existing, true));
        // file:// URIs check the same on-disk path (the raw string never
        // exists — every boot would re-seed).
        assert!(!should_seed(&format!("file://{existing}"), false));
        assert!(should_seed(&format!("file://{missing}"), false));
    }

    #[test]
    fn force_seed_classifier_pins_the_production_values() {
        assert_eq!(force_seed_value("1"), Some(true));
        assert_eq!(force_seed_value("true"), Some(true));
        assert_eq!(force_seed_value("yes"), Some(true));
        assert_eq!(force_seed_value("0"), Some(false));
        assert_eq!(force_seed_value("false"), Some(false));
        assert_eq!(force_seed_value("no"), Some(false));
        for rejected in ["yes please", "TRUE", "on", ""] {
            assert_eq!(force_seed_value(rejected), None, "{rejected:?}");
        }
    }

    #[test]
    fn seed_step_conflict_fails_at_parse_time_naming_the_step() {
        // The both-set conflict is a parse error (steps are valid by
        // construction from there on): a bad plan never reaches the
        // execution loop, so it can never leave a half-seeded database
        // that would skip seeding on the next boot.
        let err = serde_json::from_str::<SeedConfig>(
            r#"{"steps": [{"query": "MATCH …", "load": "v=https://x", "pull": "/config/neo4j.env"}]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("exactly one of"), "{err}");
        assert!(err.to_string().contains("MATCH …"), "{err}");
    }

    #[test]
    fn instance_uris_are_suffixed_per_instance() {
        assert_eq!(suffix_uri("/data/graph", 1), "/data/graph-1");
        assert_eq!(suffix_uri("/data/graph.rl", 3), "/data/graph-3.rl");
        // A legacy v0.16 bundle keeps the extension it was stored under.
        assert_eq!(suffix_uri("/data/graph.cl", 3), "/data/graph-3.cl");
        assert_eq!(suffix_uri(":memory:", 2), ":memory:");
    }

    #[test]
    fn seed_json_parses_all_step_kinds_and_rejects_unknown_fields() {
        let config: SeedConfig = serde_json::from_str(
            r#"{"force": true, "steps": [
                {"query": "CREATE (:A)"},
                {"load": "v=https://x", "query": "UNWIND $v AS n CREATE (:B)"},
                {"pull": "/config/neo4j.env", "query": "MATCH (n) RETURN n"}
            ]}"#,
        )
        .unwrap();
        assert!(config.force);
        assert_eq!(config.steps.len(), 3);
        assert!(matches!(config.steps[0], SeedStep::Query { .. }));
        assert!(matches!(config.steps[1], SeedStep::Load { .. }));
        assert!(matches!(config.steps[2], SeedStep::Pull { .. }));
        assert!(serde_json::from_str::<SeedConfig>(r#"{"steps": 3}"#).is_err());
        // Unknown fields are still rejected (untagged enums can't do this).
        assert!(serde_json::from_str::<SeedStep>(r#"{"query": "…", "extra": 1}"#).is_err());
    }

    #[test]
    fn help_offers_load_flag_only_for_a_real_load_flag() {
        // A build without the flag (today: only --load-csv-dir) must not
        // satisfy the guard — the substring check this replaces did.
        assert!(!help_offers_load_flag(
            "  --load-csv-dir PATH  Allow local LOAD CSV under this directory\n  --bind ADDR\n"
        ));
        assert!(help_offers_load_flag(
            "  --load NAME=SOURCE  Load a JSON value\n"
        ));
        assert!(help_offers_load_flag("--load\n"));
        assert!(!help_offers_load_flag(""));
    }

    #[test]
    fn missing_config_file_falls_back_to_env_seeding() {
        // read_config on a missing path is None, so seed_instance takes the
        // env fallback — the log line promises exactly that.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.json");
        assert!(read_config(missing.to_str().unwrap()).is_none());

        let file = dir.path().join("seed.json");
        std::fs::write(&file, r#"{"steps": [{"query": "RETURN 1"}]}"#).unwrap();
        let config = read_config(file.to_str().unwrap()).unwrap();
        assert_eq!(config.plan_summary(), "1 step(s)");
    }
}
