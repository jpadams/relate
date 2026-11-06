//! `relate pull [--from FILE] --query Q [--param N=V] --database PATH`
//! (ADR-0080, ADR-0158, ADR-0200). `Q` is a Cypher statement, a `;`-separated
//! statement list, a file path, or `-` for stdin; every statement runs
//! streamed in one process with one checkpoint. The destination is
//! `--database`, not a positional.

use std::collections::BTreeMap;

use crate::parse_error::ParseError;
use relate::Database;
use relate_load::{load_streaming, resolve_shape_query, Credentials, LoadError, LoadOpts};
use serde_json::Value as Json;

/// Parsed `pull` command (flags + the required database path).
#[derive(Debug, Clone)]
pub struct PullCmd {
    pub from: Option<String>,
    pub query: String,
    pub params: BTreeMap<String, Json>,
    pub database: String,
    pub allow_plaintext: bool,
}

/// Parse the argument tail after the `pull` verb. A usage error is an `Err`
/// string for `flags::usage_error` to print as the `pull` synopsis line plus a
/// `Run 'relate pull --help' for more information.` hint, then exit 2.
pub fn parse_pull_args(rest: &[String]) -> Result<PullCmd, ParseError> {
    let mut from = None;
    let mut query = None;
    let mut params = BTreeMap::new();
    let mut database = None;
    let mut allow_plaintext = false;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--from" => {
                from = Some(crate::parse_error::flag_value(&mut args, "--from", "a path")?.clone());
            }
            "--allow-plaintext" => {
                allow_plaintext = true;
            }
            "--query" => {
                query = Some(
                    crate::parse_error::flag_value(
                        &mut args,
                        "--query",
                        "a Cypher statement or file path",
                    )?
                    .clone(),
                );
            }
            "--param" => {
                let v =
                    crate::parse_error::flag_value(&mut args, "--param", "a value (name=value)")?;
                let (k, val) = crate::params::parse_param_assignment(v)?;
                params.insert(k, val);
            }
            "-d" | "--database" => {
                database = Some(crate::parse_error::database_flag(&mut args)?.clone());
            }
            other => {
                return Err(ParseError::unexpected("pull", other));
            }
        }
    }
    let query = query.ok_or_else(|| "pull requires --query".to_string())?;
    let database = database.ok_or_else(|| "pull requires --database".to_string())?;
    if relate_storage::is_memory_uri(&database) {
        return Err("pull cannot target :memory: (--database must persist)".into());
    }
    Ok(PullCmd {
        from,
        query,
        params,
        database,
        allow_plaintext,
    })
}

/// Run `pull`. Returns the process exit code (0 success, 1 runtime).
pub fn run(cmd: PullCmd) -> i32 {
    match run_inner(cmd) {
        Ok(()) => 0,
        Err(e) => {
            crate::print_error(format_args!("Error: {e}"));
            1
        }
    }
}

fn run_inner(cmd: PullCmd) -> Result<(), LoadError> {
    let shape = resolve_shape_query(&cmd.query)?;
    let credentials = match cmd.from.as_deref() {
        Some(path) => Credentials::from_env_file(path)?,
        None => Credentials::from_process_env()?,
    };
    let db = Database::open(&cmd.database).map_err(|e| LoadError::Engine(e.to_string()))?;
    let stats = load_streaming(
        &LoadOpts {
            credentials,
            // May carry a multi-statement list (ADR-0158); every
            // statement runs streamed, one checkpoint at the end.
            query: shape,
            params: cmd.params.into_iter().collect(),
            allow_plaintext: cmd.allow_plaintext,
        },
        &db,
    )?;
    println!(
        "pulled {} new / {} reused nodes, {} new / {} reused rels \
         (+ {} constraints, {} indexes; {} schema skipped) to {}",
        stats.nodes_new,
        stats.nodes_reused,
        stats.rels_new,
        stats.rels_reused,
        stats.schema.constraints_applied,
        stats.schema.indexes_applied,
        stats.schema.constraints_skipped + stats.schema.indexes_skipped,
        cmd.database
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args;

    #[test]
    fn parse_requires_query_and_database() {
        assert!(parse_pull_args(&args(&["--query", "MATCH (n) RETURN n"]))
            .unwrap_err()
            .to_string()
            .contains("--database"));
        assert!(parse_pull_args(&args(&["-d", "./graph"]))
            .unwrap_err()
            .to_string()
            .contains("--query"));
        assert!(parse_pull_args(&args(&["--database", "./graph"]))
            .unwrap_err()
            .to_string()
            .contains("--query"));
    }

    #[test]
    fn parse_rejects_memory() {
        let err = parse_pull_args(&args(&[
            "--query",
            "MATCH (n) RETURN n",
            "--database",
            ":memory:",
        ]))
        .unwrap_err()
        .to_string();
        assert!(err.contains(":memory:"));
    }

    #[test]
    fn parse_rejects_positional_database() {
        assert_eq!(
            parse_pull_args(&args(&["--query", "MATCH (n) RETURN n", "./graph"])).unwrap_err(),
            ParseError::unexpected("pull", "./graph")
        );
    }

    #[test]
    fn parse_accepts_flags_in_any_order() {
        let cmd = parse_pull_args(&args(&[
            "--database",
            "./graph",
            "--from",
            "/tmp/n.env",
            "--query",
            "MATCH (n) RETURN n",
            "--param",
            "limit=10",
            "--allow-plaintext",
        ]))
        .unwrap();
        assert_eq!(cmd.database, "./graph");
        assert_eq!(cmd.from.as_deref(), Some("/tmp/n.env"));
        assert_eq!(cmd.params.get("limit"), Some(&Json::from(10)));
        assert!(cmd.allow_plaintext);
    }

    #[test]
    fn parse_short_database_flag_sets_the_destination() {
        let cmd =
            parse_pull_args(&args(&["--query", "MATCH (n) RETURN n", "-d", "./graph"])).unwrap();
        assert_eq!(cmd.database, "./graph");
    }

    #[test]
    fn parse_defaults_allow_plaintext_off() {
        let cmd = parse_pull_args(&args(&[
            "--query",
            "MATCH (n) RETURN n",
            "--database",
            "./graph",
        ]))
        .unwrap();
        assert!(!cmd.allow_plaintext);
    }

    #[test]
    fn parse_allow_plaintext_takes_no_value() {
        // The next token is the database flag, not the flag's value.
        let cmd = parse_pull_args(&args(&[
            "--allow-plaintext",
            "--query",
            "MATCH (n) RETURN n",
            "--database",
            "./graph",
        ]))
        .unwrap();
        assert!(cmd.allow_plaintext);
        assert_eq!(cmd.database, "./graph");
    }

    #[test]
    fn parse_rejects_unknown_flag() {
        assert_eq!(
            parse_pull_args(&args(&["--bogus", "--database", "./g"])).unwrap_err(),
            ParseError::unexpected("pull", "--bogus")
        );
    }

    #[test]
    fn parse_rejects_extra_positional() {
        assert_eq!(
            parse_pull_args(&args(&[
                "--query",
                "MATCH (n) RETURN n",
                "--database",
                "./a",
                "./b",
            ]))
            .unwrap_err(),
            ParseError::unexpected("pull", "./b")
        );
    }
}
