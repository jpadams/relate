//! The `serve` parse grammar: surface selection, value parsers, the
//! once-only folds, and the cross-flag validations that are not the
//! unbound-surface policy.

use crate::args;
use crate::serve_args::*;

#[test]
fn parse_requires_database() {
    // The `serve requires --database` error fires in the arg loop,
    // before the nothing-to-serve check — surface-independent, so the
    // test carries no surface flag and runs in every bundle, bare
    // `serve` included.
    let err = parse_serve_args(&args(&["--read-only"]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("--database"), "{err}");
}

#[test]
fn parse_rejects_a_run_with_no_surface() {
    let err = parse_serve_args(&args(&["--database", ":memory:"]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("nothing to serve"), "{err}");
    // the hint names exactly the compiled surfaces; the empty
    // vocabulary gets the authored message instead (LABS-638)
    #[cfg(feature = "surface")]
    assert!(err.contains(&*surface_hint()), "{err}");
    #[cfg(not(feature = "surface"))]
    assert!(err.contains("compiled no serve surface"), "{err}");
}

#[cfg(all(feature = "http", feature = "mcp"))]
#[test]
fn parse_both_surfaces_and_auth() {
    let cmd = parse_serve_args(&args(&[
        "--database",
        "./db",
        "--http",
        "7474",
        "--mcp",
        "8000",
        "--auth",
        "neo4j:pw",
    ]))
    .unwrap();
    assert_eq!(cmd.database, "./db");
    assert_eq!(cmd.http_port, Some(7474));
    assert_eq!(cmd.mcp, Some(McpMode::Http(8000)));
    assert_eq!(cmd.auth.as_deref(), Some("neo4j:pw"));
}

#[cfg(feature = "mcp")]
#[test]
fn parse_mcp_stdio_selects_the_stdio_transport() {
    // LABS-381, ADR-0368: the `stdio` keyword is the stdio transport; a typo is a
    // loud usage error, never a silent port-or-stdio default.
    let cmd = parse_serve_args(&args(&["--database", "db", "--mcp", "stdio"])).unwrap();
    assert_eq!(cmd.mcp, Some(McpMode::Stdio));
    for bad in ["stio", "standrd", "STDIO", "stdio-port"] {
        let err = parse_serve_args(&args(&["--database", "db", "--mcp", bad]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("--mcp"), "{bad:?}: {err}");
    }
    // The union is once-only: `--mcp stdio` plus `--mcp PORT` is a repeat
    // of the one MCP transport flag, in either order.
    for rest in [
        &["--database", "db", "--mcp", "stdio", "--mcp", "8000"][..],
        &["--database", "db", "--mcp", "8000", "--mcp", "stdio"][..],
    ] {
        let err = parse_serve_args(&args(rest)).unwrap_err().to_string();
        assert!(err.contains("--mcp"), "{rest:?}: {err}");
        assert!(err.contains("only once"), "{rest:?}: {err}");
    }
    // A port value is still the streamable-HTTP transport.
    let cmd = parse_serve_args(&args(&["--database", "db", "--mcp", "8000"])).unwrap();
    assert_eq!(cmd.mcp, Some(McpMode::Http(8000)));
}

#[test]
fn parse_marks_the_listener_fact() {
    // The `has_listener` fact is "at least one listener surface binds":
    // the mcp stdio transport binds none, every port surface binds one.
    // Each bundle asserts the surfaces it compiles in.
    #[cfg(feature = "mcp")]
    {
        let cmd = parse_serve_args(&args(&["--database", "db", "--mcp", "stdio"])).unwrap();
        assert!(!cmd.has_listener(), "--mcp stdio binds no listener");
        let cmd = parse_serve_args(&args(&["--database", "db", "--mcp", "8000"])).unwrap();
        assert!(cmd.has_listener(), "--mcp PORT binds a listener");
    }
    #[cfg(feature = "http")]
    {
        let cmd = parse_serve_args(&args(&["--database", "db", "--http", "7474"])).unwrap();
        assert!(cmd.has_listener(), "--http binds a listener");
    }
    #[cfg(feature = "bolt")]
    {
        let cmd = parse_serve_args(&args(&["--database", "db", "--bolt", "7687"])).unwrap();
        assert!(cmd.has_listener(), "--bolt binds a listener");
    }
}

#[test]
fn parse_marks_the_any_surface_fact() {
    // The `has_surface` fact is "anything at all is serving" (round 8,
    // F-8-1): the mcp stdio transport counts — it is a surface, the
    // non-listener half of the fold — where `has_listener` reads the
    // listener half alone. Each bundle asserts the surfaces it compiles
    // in.
    #[cfg(feature = "mcp")]
    {
        let cmd = parse_serve_args(&args(&["--database", "db", "--mcp", "stdio"])).unwrap();
        assert!(cmd.has_surface(), "--mcp stdio is a surface");
    }
    #[cfg(feature = "http")]
    {
        let cmd = parse_serve_args(&args(&["--database", "db", "--http", "7474"])).unwrap();
        assert!(cmd.has_surface(), "--http is a surface");
    }
    #[cfg(feature = "bolt")]
    {
        let cmd = parse_serve_args(&args(&["--database", "db", "--bolt", "7687"])).unwrap();
        assert!(cmd.has_surface(), "--bolt is a surface");
    }
}

#[cfg(feature = "mcp")]
#[test]
fn parse_marks_the_stdio_only_fact() {
    // The stdio-only fact is the one fold of the mcp stdio transport AND
    // no listener surface ([`ServeCmd::is_stdio_only`]): true only for
    // the stdio transport alone, false beside any listener surface and
    // for any port surface run.
    let cmd = parse_serve_args(&args(&["--database", "db", "--mcp", "stdio"])).unwrap();
    assert!(
        cmd.is_stdio_only(),
        "the stdio transport alone is stdio-only"
    );
    let cmd = parse_serve_args(&args(&["--database", "db", "--mcp", "8000"])).unwrap();
    assert!(!cmd.is_stdio_only(), "--mcp PORT is not stdio-only");
    #[cfg(feature = "http")]
    {
        let cmd = parse_serve_args(&args(&[
            "--database",
            "db",
            "--mcp",
            "stdio",
            "--http",
            "7474",
        ]))
        .unwrap();
        assert!(
            !cmd.is_stdio_only(),
            "a listener beside stdio is not stdio-only"
        );
    }
}

#[cfg(feature = "surface")]
#[test]
fn parse_rejects_a_bad_or_repeated_port() {
    // The one grammar policy for every compiled-in surface port flag
    // (the one compiled-surfaces list, [`SURFACE_FLAGS`]): a non-numeric
    // or out-of-range value, a missing value, and a repeat are all usage
    // errors, whichever surface serves.
    for surface in SURFACE_FLAGS {
        let flag = surface.flag;
        for rest in [
            &["--database", "db", flag, "abc"][..],
            &["--database", "db", flag, "70000"][..],
            &["--database", "db", flag][..],
        ] {
            assert!(parse_serve_args(&args(rest)).is_err(), "{rest:?}");
        }
        let err = parse_serve_args(&args(&["--database", "db", flag, "1", flag, "2"]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("only once"), "{err}");
    }
}

#[test]
fn parse_rejects_a_repeated_database() {
    // --database parks through the one once-only policy like every
    // value flag (round 1, F-1-4): a repeat is a loud usage error,
    // never a silent last-wins. The repeat fires inside the arg loop,
    // before any surface validation — so the test needs no surface
    // flag and runs in every bundle, bare `serve` included.
    let err = parse_serve_args(&args(&["--database", "db", "--database", "db2"]))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("serve --database may be given only once"),
        "{err}"
    );
}

#[test]
fn parse_rejects_token_admission() {
    // The rejection happens in the arg loop's unknown-flag arm, before
    // any surface validation — surface-independent, so the test carries
    // no surface flag and runs in every bundle, bare `serve` included.
    let err = parse_serve_args(&args(&["--database", "db", "--token-admission"]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("--token-admission"), "{err}");
}

#[cfg(feature = "surface")]
#[test]
fn parse_host_defaults_to_loopback_and_is_set_once() {
    let surface = any_surface_flag();
    let cmd = parse_serve_args(&args(&["--database", "db", surface, "7474"])).unwrap();
    assert_eq!(cmd.host, DEFAULT_HOST);
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "7474",
        "--host",
        "0.0.0.0",
    ]))
    .unwrap();
    assert_eq!(cmd.host, "0.0.0.0");
    let err = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "7474",
        "--host",
        "0.0.0.0",
        "--host",
        "::1",
    ]))
    .unwrap_err()
    .to_string();
    assert!(err.contains("--host"), "{err}");
    assert!(err.contains("only once"), "{err}");
    assert!(parse_serve_args(&args(&["--database", "db", surface, "7474", "--host"])).is_err());
}

#[cfg(feature = "surface")]
#[test]
fn parse_host_normalizes_the_bracketed_ipv6_spelling() {
    // The bracketed IPv6 spelling (`[::1]`) is URL spelling, not a bind
    // spelling (round 4, F-4-7): the parse normalizes it to the bare
    // literal — the one spelling the bind, the loopback predicate, and
    // the rendered URLs agree on — so `--host [::1]` and `--host ::1`
    // are the same command.
    let surface = any_surface_flag();
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "7474",
        "--host",
        "[::1]",
    ]))
    .unwrap();
    assert_eq!(cmd.host, "::1");
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "7474",
        "--host",
        "::1",
    ]))
    .unwrap();
    assert_eq!(cmd.host, "::1");
    // A bracketed non-IPv6 value is not a host spelling; it passes
    // through unchanged, and the bind is the loud failure.
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "7474",
        "--host",
        "[wat]",
    ]))
    .unwrap();
    assert_eq!(cmd.host, "[wat]");
}

#[cfg(feature = "http")]
#[test]
fn parse_origin_accumulates_and_validates() {
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        "--http",
        "7474",
        "--origin",
        "https://browser.neo4j.io",
        "--origin",
        "https://tool.example",
    ]))
    .unwrap();
    // The grammar's typed values flow into the command (round 4, F-4-2).
    assert_eq!(
        cmd.origins,
        [
            OriginFlagValue::Url("https://browser.neo4j.io".to_string()),
            OriginFlagValue::Url("https://tool.example".to_string()),
        ]
    );
    // `*` is allowed; a bare host is a usage error.
    let cmd =
        parse_serve_args(&args(&["--database", "db", "--http", "1", "--origin", "*"])).unwrap();
    assert_eq!(cmd.origins, [OriginFlagValue::Wildcard]);
    for bad in ["browser.neo4j.io", "localhost:3000"] {
        let err = parse_serve_args(&args(&["--database", "db", "--http", "1", "--origin", bad]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("--origin"), "{err}");
    }
}

#[cfg(feature = "bolt")]
#[test]
fn parse_bolt_identity_is_set_once_and_defaults_to_none() {
    let cmd = parse_serve_args(&args(&["--database", "db", "--bolt", "7687"])).unwrap();
    assert_eq!(cmd.bolt_identity, None);
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        "--bolt",
        "7687",
        "--bolt-identity",
        "Neo4j/5.26.0",
    ]))
    .unwrap();
    assert_eq!(cmd.bolt_identity.as_deref(), Some("Neo4j/5.26.0"));
    for rest in [
        &["--database", "db", "--bolt", "7687", "--bolt-identity"][..],
        &[
            "--database",
            "db",
            "--bolt",
            "7687",
            "--bolt-identity",
            "Neo4j/5.26.0",
            "--bolt-identity",
            "Neo4j/4.4.0",
        ][..],
    ] {
        assert!(parse_serve_args(&args(rest)).is_err(), "{rest:?}");
    }
}

#[cfg(feature = "bolt")]
#[test]
fn parse_ws_max_connections_is_an_once_only_count() {
    // ADR-0459: the WebSocket session cap is opt-in — omitted is None (the
    // resolver then reads the environment, then unlimited).
    let cmd = parse_serve_args(&args(&["--database", "db", "--bolt", "7687"])).unwrap();
    assert_eq!(cmd.ws_max_connections, None);
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        "--bolt",
        "7687",
        "--ws-max-connections",
        "4",
    ]))
    .unwrap();
    assert_eq!(cmd.ws_max_connections, Some(4));
    for rest in [
        &["--database", "db", "--bolt", "7687", "--ws-max-connections"][..],
        &[
            "--database",
            "db",
            "--bolt",
            "7687",
            "--ws-max-connections",
            "4",
            "--ws-max-connections",
            "5",
        ][..],
        &[
            "--database",
            "db",
            "--bolt",
            "7687",
            "--ws-max-connections",
            "wat",
        ][..],
    ] {
        assert!(parse_serve_args(&args(rest)).is_err(), "{rest:?}");
    }
}

#[cfg(feature = "bolt")]
#[test]
fn parse_ws_allow_origin_is_repeatable_and_canonical() {
    // ADR-0459: the bolt surface's origin flag is repeatable and parses
    // through the shared grammar, storing the canonical spelling.
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        "--bolt",
        "7687",
        "--ws-allow-origin",
        "HTTP://APP.example.com:7474/",
        "--ws-allow-origin",
        "*",
    ]))
    .unwrap();
    assert_eq!(
        cmd.ws_allow_origins,
        [
            relate_serve::origin::OriginFlagValue::Url("http://app.example.com:7474".to_string()),
            relate_serve::origin::OriginFlagValue::Wildcard,
        ]
    );
    let err = parse_serve_args(&args(&[
        "--database",
        "db",
        "--bolt",
        "7687",
        "--ws-allow-origin",
        "localhost",
    ]))
    .unwrap_err()
    .to_string();
    assert!(err.contains("--ws-allow-origin"), "{err}");
}

#[cfg(any(feature = "http", feature = "mcp"))]
#[test]
fn parse_admission_deadlines_are_once_only_counts() {
    // ADR-0451: both deadlines ride the same HTTP-based listeners and the
    // same once-only count grammar — one statement for the pair, asserted
    // directly on the fields.
    let surface = any_surface_flag();
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "0",
        "--header-deadline-secs",
        "4",
        "--body-deadline-secs",
        "30",
    ]))
    .unwrap();
    assert_eq!(cmd.header_deadline_secs, Some(4));
    assert_eq!(cmd.body_deadline_secs, Some(30));
    let plain = parse_serve_args(&args(&["--database", "db", surface, "0"])).unwrap();
    assert_eq!(plain.header_deadline_secs, None);
    assert_eq!(plain.body_deadline_secs, None);
    for (rest, flag) in [
        (
            &[
                "--database",
                "db",
                surface,
                "0",
                "--header-deadline-secs",
                "4",
                "--header-deadline-secs",
                "5",
            ][..],
            "--header-deadline-secs",
        ),
        (
            &[
                "--database",
                "db",
                surface,
                "0",
                "--body-deadline-secs",
                "30",
                "--body-deadline-secs",
                "60",
            ][..],
            "--body-deadline-secs",
        ),
    ] {
        let err = parse_serve_args(&args(rest)).unwrap_err().to_string();
        assert!(err.contains(flag), "{flag}: {err}");
        assert!(err.contains("only once"), "{flag}: {err}");
    }
    for (rest, flag) in [
        (
            &[
                "--database",
                "db",
                surface,
                "0",
                "--header-deadline-secs",
                "soon",
            ][..],
            "--header-deadline-secs",
        ),
        (
            &[
                "--database",
                "db",
                surface,
                "0",
                "--body-deadline-secs",
                "soon",
            ][..],
            "--body-deadline-secs",
        ),
    ] {
        let err = parse_serve_args(&args(rest)).unwrap_err().to_string();
        assert!(err.contains(flag), "{flag}: {err}");
    }
}

#[cfg(feature = "surface")]
#[test]
fn parse_processing_controls() {
    let surface = any_surface_flag();
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--max-connections",
        "4",
        "--concurrent-logical-workspace-limit",
        "2mib",
    ]))
    .unwrap();
    assert_eq!(cmd.max_connections, Some(4));
    assert_eq!(
        cmd.concurrent_logical_workspace_limit,
        Some(2 * 1024 * 1024)
    );
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--max-active-queries",
        "2",
    ]))
    .is_err());
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--not-a-serve-flag",
        "1mib",
    ]))
    .is_err());
}

#[cfg(feature = "http")]
#[test]
fn parse_tx_lifecycle_limits() {
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        "--http",
        "7474",
        "--tx-idle-timeout-secs",
        "30",
        "--max-open-transactions",
        "64",
    ]))
    .unwrap();
    assert_eq!(cmd.tx_idle_timeout_secs, Some(30));
    assert_eq!(cmd.max_open_transactions, Some(64));
    let plain = parse_serve_args(&args(&["--database", "db", "--http", "7474"])).unwrap();
    assert_eq!(plain.tx_idle_timeout_secs, None);
    assert_eq!(plain.max_open_transactions, None);
    for rest in [
        &[
            "--database",
            "db",
            "--http",
            "7474",
            "--tx-idle-timeout-secs",
            "30",
            "--tx-idle-timeout-secs",
            "60",
        ][..],
        &[
            "--database",
            "db",
            "--http",
            "7474",
            "--max-open-transactions",
            "64",
            "--max-open-transactions",
            "128",
        ][..],
        &[
            "--database",
            "db",
            "--http",
            "7474",
            "--tx-idle-timeout-secs",
            "soon",
        ][..],
        &[
            "--database",
            "db",
            "--http",
            "7474",
            "--max-open-transactions",
            "-1",
        ][..],
    ] {
        assert!(parse_serve_args(&args(rest)).is_err(), "{rest:?}");
    }
}

#[cfg(feature = "surface")]
#[test]
fn parse_expression_memory_limit_takes_the_size_grammar_and_none() {
    let surface = any_surface_flag();
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--expression-memory-limit",
        "256mib",
    ]))
    .unwrap();
    assert_eq!(cmd.expression_memory_limit, Some(256 << 20));
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--expression-memory-limit",
        "none",
    ]))
    .unwrap();
    assert_eq!(cmd.expression_memory_limit, Some(0));
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--expression-memory-limit",
        "1tb",
    ]))
    .is_err());
}

#[cfg(any(feature = "http", feature = "mcp", feature = "bolt"))]
#[test]
fn parse_logical_graph_size_limit_takes_the_query_paths_value_grammar() {
    let surface = any_surface_flag();
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--logical-graph-size-limit",
        "2mib",
    ]))
    .unwrap();
    assert_eq!(cmd.logical_graph_size_limit, Some(2 * 1024 * 1024));
    // A bare byte count parses too.
    let cmd = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--logical-graph-size-limit",
        "1048576",
    ]))
    .unwrap();
    assert_eq!(cmd.logical_graph_size_limit, Some(1024 * 1024));
    // A bad size is a usage error, not a silent ignore.
    let err = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--logical-graph-size-limit",
        "wat",
    ]))
    .unwrap_err()
    .to_string();
    assert!(err.contains("--logical-graph-size-limit"), "{err}");
    // Once-only, like every value-taking serve flag.
    let err = parse_serve_args(&args(&[
        "--database",
        "db",
        surface,
        "1",
        "--logical-graph-size-limit",
        "1mib",
        "--logical-graph-size-limit",
        "2mib",
    ]))
    .unwrap_err()
    .to_string();
    assert!(err.contains("only once"), "{err}");
}

/// ADR-0470: `--durability-mode` takes the engine's mode grammar
/// (`DurabilityMode::from_str`, the query path's flag). The value-error
/// halves run in every serve build — the errors fire in the arg loop,
/// before the nothing-to-serve check — so the bare `serve` bundle pins
/// them too.
#[test]
fn parse_durability_mode_rejects_a_bad_value_in_every_serve_build() {
    let err = parse_serve_args(&args(&["--database", "db", "--durability-mode", "later"]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("--durability-mode"), "{err}");
    assert!(
        err.contains("expected full, everysecond, or snapshot"),
        "{err}"
    );
    // Once-only, like every value-taking serve flag.
    let err = parse_serve_args(&args(&[
        "--database",
        "db",
        "--durability-mode",
        "full",
        "--durability-mode",
        "snapshot",
    ]))
    .unwrap_err()
    .to_string();
    assert!(err.contains("only once"), "{err}");
}

/// ADR-0470: each mode parses into the typed value; omitted defers to the
/// environment, then the engine default — the shared resolution.
#[cfg(feature = "surface")]
#[test]
fn parse_durability_mode_takes_the_query_paths_value_grammar() {
    let surface = any_surface_flag();
    for &(value, mode) in relate_test_support::durability::DURABILITY_MODES {
        let cmd = parse_serve_args(&args(&[
            "--database",
            "db",
            surface,
            "1",
            "--durability-mode",
            value,
        ]))
        .unwrap();
        assert_eq!(cmd.durability_mode, Some(mode), "{value}");
    }
    let cmd = parse_serve_args(&args(&["--database", "db", surface, "1"])).unwrap();
    assert_eq!(cmd.durability_mode, None);
}

#[test]
fn parse_rejects_unknown_flags_and_positionals() {
    for rest in [
        // `--bolt` is a real surface flag now; pick a genuinely unknown
        // one for the rejection pin.
        &["--database", "db", "--wat", "7687"][..],
        &["--database", "db", "extra", "--http", "7474"][..],
    ] {
        let err = parse_serve_args(&args(rest)).unwrap_err();
        assert!(
            matches!(err, ParseError::Rejected { .. }),
            "{rest:?}: {err}"
        );
    }
}

#[cfg(feature = "surface")]
#[test]
fn parse_selects_the_open_mode_from_the_freshness_value() {
    // ADR-0480: one access flag — bare `--read-only` opens the frozen
    // snapshot (ADR-0136), `--read-only=live` tails the WAL (ADR-0137, the
    // former `--follow`), `--read-only=snapshot` names the default, and no
    // flag stays writable. The value is attached-form only (the shared
    // sighting parse's refusal is pinned in parse_error's own tests).
    let surface = any_surface_flag();
    let parse_access = |extra: &[&str]| {
        let mut rest = vec!["--database", "db", surface, "7474"];
        rest.extend_from_slice(extra);
        parse_serve_args(&args(&rest)).unwrap().access
    };
    assert_eq!(parse_access(&[]), relate::OpenMode::Writable);
    assert_eq!(parse_access(&["--read-only"]), relate::OpenMode::ReadOnly);
    assert_eq!(
        parse_access(&["--read-only=snapshot"]),
        relate::OpenMode::ReadOnly
    );
    assert_eq!(
        parse_access(&["--read-only=live"]),
        relate::OpenMode::Follow
    );
}

#[test]
fn parse_rejects_a_repeated_access_flag() {
    // The once-only policy (ADR-0480 §4, parse_error's `set_access_once`): a
    // repeat of the one flag is a usage error naming it — including a
    // conflicting second value, which is the same flag's repeat. Runs in
    // the arg loop, before the nothing-to-serve check —
    // surface-independent, so the test carries no surface flag and runs in
    // every bundle, bare `serve` included.
    for extra in [
        &["--read-only", "--read-only"][..],
        &["--read-only=live", "--read-only=snapshot"][..],
        &["--read-only=live", "--read-only=live"][..],
    ] {
        let mut rest = vec!["--database", "db"];
        rest.extend_from_slice(extra);
        let err = parse_serve_args(&args(&rest)).unwrap_err().to_string();
        assert!(err.contains("--read-only"), "{extra:?}: {err}");
        assert!(err.contains("only once"), "{extra:?}: {err}");
    }
}

/// LABS-680 (ADR-0477): `--safe`/`--no-safe` are value-less and the last one
/// wins; absent leaves `None` for the environment ladder in setup.
#[cfg(feature = "mcp")]
#[test]
fn parse_safe_and_no_safe_last_flag_wins() {
    let base = ["--database", "db", "--mcp", "stdio"];
    let parse = |extra: &[&str]| {
        let mut rest = base.to_vec();
        rest.extend_from_slice(extra);
        parse_serve_args(&args(&rest)).unwrap().safe
    };
    assert_eq!(parse(&[]), None);
    assert_eq!(parse(&["--safe"]), Some(true));
    assert_eq!(parse(&["--no-safe"]), Some(false));
    assert_eq!(parse(&["--safe", "--no-safe"]), Some(false));
    assert_eq!(parse(&["--no-safe", "--safe"]), Some(true));
}

#[test]
fn parse_rejects_a_space_separated_freshness_value() {
    // ADR-0480 §3: the value is attached-form only — `--read-only live`
    // must never let `live` silently become a path or a query; it is a
    // usage error naming the attached spelling.
    let rest = vec!["--database", "db", "--read-only", "live"];
    let err = parse_serve_args(&args(&rest)).unwrap_err().to_string();
    assert!(
        err.contains("--read-only takes its value attached: --read-only=live"),
        "{err}"
    );
}
