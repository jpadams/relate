//! argv tokenization, redaction, and image-default resolution — the one
//! parse of the container's argv that every entrypoint consumer shares.
//!
//! ADR-0249: the entrypoint now drives `relate serve` (Bolt + HTTP +
//! MCP in one process). The container's arg surface is serve's flag model
//! (`--database`, `--host`, `--bolt`/`--http`/`--mcp PORT`, `--auth`),
//! with the ADR-0128 bolt-binary spellings accepted as legacy aliases:
//! `--uri` ≡ `--database`, `--bind HOST:PORT` ≡ `--host HOST --bolt
//! PORT`, and `--allow-plaintext` is accepted and dropped (serve is
//! plaintext-only; a non-loopback `--host` is itself the deliberate,
//! loudly-warned exposure, ADR-0248).

/// The image's default bind host. `serve`'s own default is loopback
/// (right for a host binary, unreachable from outside a container), so
/// both serve paths resolve the effective host through this.
pub(crate) const DEFAULT_HOST: &str = "0.0.0.0";
/// The image's default surface ports: Bolt 7687, HTTP Query API v2 7474,
/// MCP streamable-HTTP 7475 (ADR-0249).
pub(crate) const DEFAULT_BOLT_PORT: u16 = 7687;
pub(crate) const DEFAULT_HTTP_PORT: u16 = 7474;
pub(crate) const DEFAULT_MCP_PORT: u16 = 7475;

/// The resolved bind plan: one host and one port per surface. The fleet
/// path steps every port from these bases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServePlan {
    pub host: String,
    pub bolt_port: u16,
    pub http_port: u16,
    pub mcp_port: u16,
}

/// Split a legacy `--bind` value into (host, port); a bare port means all
/// interfaces. The one host:port parser for the entrypoint.
pub(crate) fn split_bind(bind: &str) -> (&str, &str) {
    match bind.rsplit_once(':') {
        Some((host, port)) => (host, port),
        None => ("0.0.0.0", bind),
    }
}

/// The loopback predicate is the shared `relate-serve` one (parse-based,
/// so `127.0.0.1.evil.com` is not loopback) — the container's warning and
/// the CLI's ADR-0248 posture read the same definition.
pub(crate) use relate_serve::{is_loopback_host, parse_port};

/// The loopback warning, one wording for both serve paths (single-serve
/// and fleet alike): the server cannot see whether the host published the
/// ports (-p is docker's job; EXPOSE is documentation only), but a loopback
/// bind is unreachable from outside the container no matter what the host
/// does — the classic "it's running but I can't connect" misconfiguration.
pub(crate) fn warn_if_loopback(host: &str) {
    if is_loopback_host(host) {
        eprintln!(
            "entrypoint: warning: --host {host} is reachable only inside the container; from the host use --host 0.0.0.0 and publish the ports (-p {DEFAULT_BOLT_PORT}:{DEFAULT_BOLT_PORT} -p {DEFAULT_HTTP_PORT}:{DEFAULT_HTTP_PORT} -p {DEFAULT_MCP_PORT}:{DEFAULT_MCP_PORT})"
        );
    }
}

/// Server args that take no value: they never consume the next token.
/// The one access flag `--read-only` (ADR-0480: bare is the snapshot
/// form — the attached `--read-only=live` splits through the `=`-arm
/// above, and the bare form must not swallow a following flag), the
/// removed `--follow` spelling (ADR-0480: forwarded verbatim as one bare
/// token so the server's unknown-flag error names it exactly), the
/// legacy bool `--allow-plaintext` (ADR-0249), and the `--help`/
/// `--version` spells take no value; every other arg forwarded to
/// `serve` takes one (the CLI is hand-rolled, so there is no clap
/// Command to derive from — the set is stated, and the tests below pin
/// it).
fn valueless_flags() -> &'static [&'static str] {
    &[
        "--allow-plaintext",
        "--read-only",
        "--follow",
        "--help",
        "--version",
    ]
}

/// One argv tokenization for every entrypoint consumer: `--flag value`
/// and `--flag=value` both become (`--flag`, Some(`value`)); `spaced`
/// remembers the spelling so pass-through re-rendering is byte-identical.
/// A bare token, a flag at the end of argv, or a valueless bool (see
/// `valueless_flags`) becomes (`token`, None). A value-taking `--flag`
/// consumes the next token even when it looks like a flag — the server's
/// own parser owns validation.
struct ArgToken<'a> {
    flag: &'a str,
    value: Option<&'a str>,
    spaced: bool,
}

fn tokenize_args(args: &[String]) -> Vec<ArgToken<'_>> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                out.push(ArgToken {
                    flag,
                    value: Some(value),
                    spaced: false,
                });
                i += 1;
            }
            _ if arg.starts_with("--")
                && !valueless_flags().contains(&arg)
                && i + 1 < args.len() =>
            {
                out.push(ArgToken {
                    flag: arg,
                    value: Some(args[i + 1].as_str()),
                    spaced: true,
                });
                i += 2;
            }
            _ => {
                out.push(ArgToken {
                    flag: arg,
                    value: None,
                    spaced: false,
                });
                i += 1;
            }
        }
    }
    out
}

/// Re-render one token to its exact argv spelling (one or two strings).
fn render_arg(token: &ArgToken<'_>, out: &mut Vec<String>) {
    match (token.value, token.spaced) {
        (Some(v), true) => {
            out.push(token.flag.to_string());
            out.push(v.to_string());
        }
        (Some(v), false) => out.push(format!("{}={v}", token.flag)),
        (None, _) => out.push(token.flag.to_string()),
    }
}

/// The flags the entrypoint consumes and rebuilds (per instance on the
/// fleet path); everything else passes through to `relate serve`.
const CONSUMED_FLAGS: &[&str] = &[
    "--database",
    "--uri",
    "--host",
    "--bind",
    "--bolt",
    "--http",
    "--mcp",
    "--allow-plaintext",
];

/// The user's args minus the entrypoint-consumed flags (`--host`/`--bolt`/
/// `--http`/`--mcp`/`--database` are rebuilt per instance; `--uri`,
/// `--bind` and `--allow-plaintext` are translated or dropped; the rest —
/// `--auth`, `--origin` — pass on).
pub(crate) fn server_extra_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for token in tokenize_args(args) {
        if CONSUMED_FLAGS.contains(&token.flag) {
            continue;
        }
        render_arg(&token, &mut out);
    }
    out
}

/// The argv for the startup banner, with secret-bearing flag values
/// redacted. `docker run image --auth user:password` is a documented way
/// in (docs/api/bolt/spec.md), and container logs outlive the run and
/// travel further than `docker inspect` — the same reason `--load`
/// sources log only their parameter name.
pub(crate) fn redacted_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    for token in tokenize_args(args) {
        if token.flag == "--auth" && token.value.is_some() {
            if token.spaced {
                out.push(token.flag.to_string());
                out.push("<redacted>".to_string());
            } else {
                out.push(format!("{}=<redacted>", token.flag));
            }
        } else {
            render_arg(&token, &mut out);
        }
    }
    out
}

/// The effective value of `--flag <v>` or `--flag=<v>` in `args`
/// (first occurrence). Test-only: production resolution goes through
/// [`single_value`]'s loud repeat check.
#[cfg(test)]
fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    tokenize_args(args)
        .into_iter()
        .find(|t| t.flag == flag)
        .and_then(|t| t.value)
}

/// The one occurrence of `flag` in `args`: a repeat is a loud error —
/// the CLI's once-only policy (`serve {flag} may be given only once`),
/// not a silent first- or last-wins, because the entrypoint rebuilds the
/// consumed flags and a collapsed repeat would otherwise never reach
/// `serve`'s own rejection.
fn single_value<'a>(args: &'a [String], flag: &str) -> Result<Option<&'a str>, String> {
    let mut values = tokenize_args(args)
        .into_iter()
        .filter(|t| t.flag == flag)
        .filter_map(|t| t.value);
    let first = values.next();
    if values.next().is_some() {
        return Err(format!("{flag} may be given only once"));
    }
    Ok(first)
}

/// A value given under a canonical flag or its legacy alias: both at once
/// is a loud error (the two spellings could name different things), a
/// repeat of either is a loud error, otherwise the one present wins.
fn canonical_or_alias<'a>(
    args: &'a [String],
    canonical: &str,
    alias: &str,
) -> Result<Option<&'a str>, String> {
    match (single_value(args, canonical)?, single_value(args, alias)?) {
        (Some(_), Some(_)) => Err(format!(
            "{canonical} and {alias} are aliases; pass only one"
        )),
        (v, None) | (None, v) => Ok(v),
    }
}

/// The store to seed and serve: `--database`, with `--uri` as the legacy
/// alias (ADR-0249). Both at once is a loud error — the two spellings
/// naming different stores would seed one and serve the other.
pub(crate) fn database_arg(args: &[String]) -> Result<Option<&str>, String> {
    canonical_or_alias(args, "--database", "--uri")
}

/// The explicitly-passed value of a port flag, or `None` when argv omits
/// it — the fleet path derives unset bases from the bolt base (ADR-0249),
/// so it must tell "unset" apart from the image default.
pub(crate) fn explicit_port(args: &[String], flag: &str) -> Result<Option<u16>, String> {
    single_value(args, flag)?
        .map(|v| parse_port(flag, v))
        .transpose()
}

/// The legacy `--allow-plaintext` note, spoken once per container start
/// (from both serve paths) rather than as a side effect of plan
/// resolution: serve is plaintext-only, so the flag is subsumed by a
/// non-loopback `--host`'s own loud warning (ADR-0248) and dropped.
pub(crate) fn note_dropped_legacy_args(args: &[String]) {
    if tokenize_args(args)
        .iter()
        .any(|t| t.flag == "--allow-plaintext")
    {
        eprintln!(
            "entrypoint: note: --allow-plaintext is subsumed by serve's --host posture (serve is plaintext-only; a non-loopback --host warns loudly) — dropping it"
        );
    }
}

/// Resolve the bind plan from argv: `--host`/`--bolt`/`--http`/`--mcp`,
/// with legacy `--bind HOST:PORT` mapping onto host + bolt port and the
/// image defaults filling the rest. Mixing a new spelling with its legacy
/// alias is a loud error, not a coin flip.
pub(crate) fn resolve_plan(args: &[String]) -> Result<ServePlan, String> {
    let (bind_host, bind_port) = match single_value(args, "--bind")? {
        Some(b) => {
            let (host, port) = split_bind(b);
            (Some(host), Some(parse_port("--bind", port)?))
        }
        None => (None, None),
    };
    let host = match (single_value(args, "--host")?, bind_host) {
        (Some(_), Some(_)) => {
            return Err("--host and --bind are aliases; pass only one".to_string());
        }
        (Some(h), None) => h.to_string(),
        (None, Some(h)) => h.to_string(),
        (None, None) => DEFAULT_HOST.to_string(),
    };
    let bolt_port = match (single_value(args, "--bolt")?, bind_port) {
        (Some(_), Some(_)) => {
            return Err("--bolt and --bind are aliases; pass only one".to_string());
        }
        (Some(p), None) => parse_port("--bolt", p)?,
        (None, Some(p)) => p,
        (None, None) => DEFAULT_BOLT_PORT,
    };
    let http_port = match explicit_port(args, "--http")? {
        Some(p) => p,
        None => DEFAULT_HTTP_PORT,
    };
    let mcp_port = match explicit_port(args, "--mcp")? {
        Some(p) => p,
        None => DEFAULT_MCP_PORT,
    };
    Ok(ServePlan {
        host,
        bolt_port,
        http_port,
        mcp_port,
    })
}

/// The argv handed to `relate serve`: the entrypoint-resolved
/// `--database`, `--host` and surface ports first (explicit, so serve's
/// own loopback/no-defaults posture never leaks into the container), then
/// the pass-through extras (`--auth`, `--origin`, …).
pub(crate) fn serve_argv(args: &[String], plan: &ServePlan, database: &str) -> Vec<String> {
    let mut out = vec![
        "serve".to_string(),
        "--database".to_string(),
        database.to_string(),
        "--host".to_string(),
        plan.host.clone(),
        "--bolt".to_string(),
        plan.bolt_port.to_string(),
        "--http".to_string(),
        plan.http_port.to_string(),
        "--mcp".to_string(),
        plan.mcp_port.to_string(),
    ];
    out.extend(server_extra_args(args));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_bind_forms() {
        assert_eq!(split_bind("0.0.0.0:7687"), ("0.0.0.0", "7687"));
        assert_eq!(split_bind("7687"), ("0.0.0.0", "7687"));
    }

    #[test]
    fn loopback_hosts_are_flagged_and_others_are_not() {
        for host in ["127.0.0.1", "localhost", "::1", "[::1]"] {
            assert!(is_loopback_host(host), "{host} should warn");
        }
        // All-interfaces and specific-address hosts are reachable from
        // outside the container — no warning.
        for host in ["0.0.0.0", "172.17.0.2"] {
            assert!(!is_loopback_host(host), "{host} should not warn");
        }
    }

    #[test]
    fn arg_value_space_and_equals_forms() {
        let args = sv(&["--database", "/data/x.rl", "--auth=user:pw"]);
        assert_eq!(arg_value(&args, "--database"), Some("/data/x.rl"));
        assert_eq!(arg_value(&args, "--auth"), Some("user:pw"));
        assert_eq!(arg_value(&args, "--host"), None);
    }

    #[test]
    fn database_arg_aliases_and_conflicts() {
        assert_eq!(
            database_arg(&sv(&["--uri", "/data/a"])).unwrap(),
            Some("/data/a")
        );
        assert_eq!(
            database_arg(&sv(&["--database", "/data/b"])).unwrap(),
            Some("/data/b")
        );
        assert!(database_arg(&sv(&["--uri", "/data/a", "--database", "/data/b"])).is_err());
        assert_eq!(database_arg(&sv(&["--auth", "none"])).unwrap(), None);
    }

    #[test]
    fn resolve_plan_defaults_and_explicit_flags() {
        let plan = resolve_plan(&sv(&["--auth", "none"])).unwrap();
        assert_eq!(
            plan,
            ServePlan {
                host: "0.0.0.0".to_string(),
                bolt_port: 7687,
                http_port: 7474,
                mcp_port: 7475,
            }
        );
        let plan = resolve_plan(&sv(&[
            "--host", "0.0.0.0", "--bolt", "17687", "--http", "17474", "--mcp", "17475",
        ]))
        .unwrap();
        assert_eq!(plan.bolt_port, 17687);
        assert_eq!(plan.http_port, 17474);
        assert_eq!(plan.mcp_port, 17475);
    }

    #[test]
    fn resolve_plan_maps_the_legacy_bind() {
        let plan = resolve_plan(&sv(&["--bind", "0.0.0.0:8687"])).unwrap();
        assert_eq!(plan.host, "0.0.0.0");
        assert_eq!(plan.bolt_port, 8687);
        // A bare port is all-interfaces.
        let plan = resolve_plan(&sv(&["--bind", "8687"])).unwrap();
        assert_eq!(plan.host, "0.0.0.0");
        assert_eq!(plan.bolt_port, 8687);
        // The new spelling mixed with its alias is a loud error.
        assert!(resolve_plan(&sv(&["--bind", "0.0.0.0:8687", "--host", "0.0.0.0"])).is_err());
        assert!(resolve_plan(&sv(&["--bind", "0.0.0.0:8687", "--bolt", "1"])).is_err());
        // A bad port names its flag.
        let err = resolve_plan(&sv(&["--bind", "0.0.0.0:abc"])).unwrap_err();
        assert!(err.contains("--bind"), "{err}");
        let err = resolve_plan(&sv(&["--mcp", "abc"])).unwrap_err();
        assert!(err.contains("--mcp"), "{err}");
    }

    #[test]
    fn serve_argv_puts_the_resolved_plan_first_and_keeps_extras() {
        let args = sv(&[
            "--bind",
            "0.0.0.0:7687",
            "--uri=/data/x.rl",
            "--auth",
            "user:pw",
            "--allow-plaintext",
            "--origin",
            "https://tool.example",
        ]);
        let plan = resolve_plan(&args).unwrap();
        assert_eq!(
            serve_argv(&args, &plan, "/data/x.rl"),
            sv(&[
                "serve",
                "--database",
                "/data/x.rl",
                "--host",
                "0.0.0.0",
                "--bolt",
                "7687",
                "--http",
                "7474",
                "--mcp",
                "7475",
                "--auth",
                "user:pw",
                "--origin",
                "https://tool.example",
            ])
        );
    }

    #[test]
    fn serve_argv_keeps_a_valueless_removed_spelling_once_with_the_default_database() {
        // argv of just --follow — the spelling ADR-0480 removed: it stays
        // valueless (one bare token, never consuming a follower), forwards
        // verbatim, and the server answers it with the unknown-flag error.
        // The appended default database (/data/graph, resolved by main.rs)
        // still lands adjacent to its flag.
        let args = sv(&["--follow"]);
        let plan = resolve_plan(&args).unwrap();
        assert_eq!(
            serve_argv(&args, &plan, "/data/graph"),
            sv(&[
                "serve",
                "--database",
                "/data/graph",
                "--host",
                "0.0.0.0",
                "--bolt",
                "7687",
                "--http",
                "7474",
                "--mcp",
                "7475",
                "--follow",
            ])
        );
    }

    #[test]
    fn serve_argv_keeps_the_attached_freshness_value_byte_identical() {
        // ADR-0480: `--read-only=live` splits through the `=`-arm and
        // re-renders attached; the bare `--read-only` never consumes a
        // following flag (it is valueless here), and the spaced
        // `--read-only live` travels as two tokens for the server's own
        // grammar to refuse.
        let args = sv(&["--read-only=live", "--read-only"]);
        let plan = resolve_plan(&args).unwrap();
        assert_eq!(
            serve_argv(&args, &plan, "/data/graph"),
            sv(&[
                "serve",
                "--database",
                "/data/graph",
                "--host",
                "0.0.0.0",
                "--bolt",
                "7687",
                "--http",
                "7474",
                "--mcp",
                "7475",
                "--read-only=live",
                "--read-only",
            ])
        );
    }

    #[test]
    fn server_extra_args_drop_rebuilt_flags_and_keep_the_rest() {
        let args = sv(&[
            "--bind",
            "0.0.0.0:7687",
            "--uri=/data/x.rl",
            "--database",
            "/data/y",
            "--host",
            "0.0.0.0",
            "--bolt",
            "1",
            "--http",
            "2",
            "--mcp",
            "3",
            "--allow-plaintext",
            "--auth",
            "none",
        ]);
        assert_eq!(
            server_extra_args(&args),
            vec!["--auth".to_string(), "none".to_string()]
        );
    }

    #[test]
    fn valueless_flags_do_not_swallow_the_next_token() {
        // --allow-plaintext is a valueless legacy bool: the URI after it
        // must survive (the entrypoint seeds the store serve serves)…
        let args = sv(&["--allow-plaintext", "--uri", "/data/x.rl"]);
        assert_eq!(arg_value(&args, "--uri"), Some("/data/x.rl"));
        assert_eq!(arg_value(&args, "--allow-plaintext"), None);
        // …and --auth stays redacted behind a bool flag, in any order.
        let args = sv(&["--allow-plaintext", "--auth", "relate:s3cret"]);
        let redacted = redacted_args(&args);
        assert!(
            !redacted.iter().any(|a| a.contains("s3cret")),
            "{redacted:?}"
        );
        assert_eq!(redacted, vec!["--allow-plaintext", "--auth", "<redacted>"]);
        // Pass-through re-rendering drops the consumed bool.
        assert_eq!(
            server_extra_args(&args),
            vec!["--auth".to_string(), "relate:s3cret".to_string()]
        );
    }

    #[test]
    fn valueless_access_flags_do_not_swallow_the_next_token() {
        // ADR-0480: `--read-only` (bare snapshot form) and the removed
        // `--follow` spelling are valueless like the legacy bool: the
        // database flag after either survives as the next token's flag —
        // a value-taking read would swallow it and break the entrypoint's
        // own seeding plan — and each passes through as one bare token
        // (the attached `--read-only=live` splits through the `=`-arm).
        let args = sv(&["--read-only", "--database", "/data/x"]);
        assert_eq!(database_arg(&args).unwrap(), Some("/data/x"));
        assert_eq!(arg_value(&args, "--read-only"), None);
        assert_eq!(server_extra_args(&args), vec!["--read-only".to_string()]);
        let args = sv(&["--follow", "--uri", "/data/x"]);
        assert_eq!(database_arg(&args).unwrap(), Some("/data/x"));
        assert_eq!(arg_value(&args, "--follow"), None);
        assert_eq!(server_extra_args(&args), vec!["--follow".to_string()]);
    }

    #[test]
    fn banner_args_redact_auth_in_both_forms() {
        let args = sv(&[
            "--host",
            "0.0.0.0",
            "--auth",
            "relate:s3cret",
            "--database=/data/g",
            "--auth=relate:s3cret",
        ]);
        let redacted = redacted_args(&args);
        assert_eq!(
            redacted,
            vec![
                "--host",
                "0.0.0.0",
                "--auth",
                "<redacted>",
                "--database=/data/g",
                "--auth=<redacted>",
            ]
        );
        assert!(!redacted.iter().any(|a| a.contains("s3cret")));
    }
}
