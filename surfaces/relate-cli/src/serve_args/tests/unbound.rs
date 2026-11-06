//! The unbound-surface flag policy: a flag that configures a surface this
//! run does not serve is a loud usage error, and the surface table it reads
//! is pinned here.

use crate::args;
use crate::serve_args::*;

#[cfg(feature = "mcp")]
#[test]
fn parse_rejects_auth_on_a_stdio_only_serve() {
    // Stdio skips the auth story (ADR-0232 §1); an explicit `--auth` on a
    // run with no listener surface is a loud usage error, not an ignored
    // flag. With a listener surface beside it, the flag applies as usual.
    let err = parse_serve_args(&args(&[
        "--database",
        "db",
        "--mcp",
        "stdio",
        "--auth",
        "a:b",
    ]))
    .unwrap_err()
    .to_string();
    assert!(err.contains("--mcp stdio"), "{err}");
    assert!(err.contains("--auth"), "{err}");
    #[cfg(feature = "http")]
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        "--mcp",
        "stdio",
        "--http",
        "7474",
        "--auth",
        "a:b"
    ]))
    .is_ok());
}

#[cfg(any(feature = "http", feature = "mcp"))]
#[test]
fn parse_rejects_admission_deadlines_without_an_http_based_listener() {
    // ADR-0451: the deadlines configure the HTTP request phases. A
    // stdio-only run binds no socket, so each value is a loud usage error,
    // not a silent discard — the same posture as the sibling flags.
    for flag in ["--header-deadline-secs", "--body-deadline-secs"] {
        #[cfg(feature = "mcp")]
        {
            let err = parse_serve_args(&args(&["--database", "db", "--mcp", "stdio", flag, "5"]))
                .unwrap_err()
                .to_string();
            assert!(err.contains(flag), "{flag}: {err}");
            assert!(err.contains("--mcp PORT"), "{flag}: {err}");
        }
        // A bound HTTP-based listener makes the flag valid — the
        // table's own classification picks it (http_header_phase), not
        // table order.
        let surface = http_based_surface_flag();
        assert!(
            parse_serve_args(&args(&["--database", "db", surface, "0", flag, "5"])).is_ok(),
            "{flag} applies beside an HTTP-based listener"
        );
    }
}

#[cfg(feature = "mcp")]
#[test]
fn parse_rejects_listener_only_flags_on_a_stdio_only_serve() {
    // The one unbound-surface flag policy (`unbound::reject`,
    // round 1, F-1-6): a stdio-only serve rejects every listener-only flag
    // loudly — each sets the story of a surface the run does not have —
    // instead of silently ignoring it. `--auth` keeps its own rejection
    // (pinned by parse_rejects_auth_on_a_stdio_only_serve); the tolerated
    // posture knobs `--host` and `--max-connections` still parse.
    #[cfg(feature = "http")]
    {
        let err = parse_serve_args(&args(&[
            "--database",
            "db",
            "--mcp",
            "stdio",
            "--origin",
            "https://tool.example",
        ]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("--origin"), "{err}");
        // Beside a listener surface the flag is not listener-only.
        assert!(parse_serve_args(&args(&[
            "--database",
            "db",
            "--mcp",
            "stdio",
            "--http",
            "7474",
            "--origin",
            "https://tool.example",
        ]))
        .is_ok());
    }
    #[cfg(feature = "bolt")]
    {
        let err = parse_serve_args(&args(&[
            "--database",
            "db",
            "--mcp",
            "stdio",
            "--bolt-identity",
            "Neo4j/5.26.0",
        ]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("--bolt-identity"), "{err}");
    }
    // The tolerated posture knobs on a stdio-only run: accepted, not
    // rejected by the policy.
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        "--mcp",
        "stdio",
        "--host",
        "0.0.0.0",
        "--max-connections",
        "4",
    ]))
    .is_ok());
}

#[cfg(all(feature = "bolt", feature = "http"))]
#[test]
fn parse_rejects_flags_of_a_surface_the_run_does_not_serve() {
    // The unbound-surface flag policy generalizes the stdio-only rejection
    // (round 1, F-1-6): a flag that configures a surface this run does not
    // serve fails loudly instead of being silently discarded. A bolt-only
    // serve handed HTTP flags and an HTTP-only serve handed a Bolt flag are
    // both usage errors — the flag expressed an intent this run cannot
    // honor (the ADR-0433/ADR-0435 posture). The environment spellings stay
    // covered by setup's resolution.
    for (rest, flag) in [
        (
            &[
                "--database",
                "db",
                "--bolt",
                "0",
                "--origin",
                "https://tool.example",
            ][..],
            "--origin",
        ),
        (
            &[
                "--database",
                "db",
                "--bolt",
                "0",
                "--tx-idle-timeout-secs",
                "5",
            ][..],
            "--tx-idle-timeout-secs",
        ),
        (
            &[
                "--database",
                "db",
                "--bolt",
                "0",
                "--max-open-transactions",
                "8",
            ][..],
            "--max-open-transactions",
        ),
        (
            &[
                "--database",
                "db",
                "--bolt",
                "0",
                "--header-deadline-secs",
                "5",
            ][..],
            "--header-deadline-secs",
        ),
        (
            &[
                "--database",
                "db",
                "--bolt",
                "0",
                "--body-deadline-secs",
                "5",
            ][..],
            "--body-deadline-secs",
        ),
        (
            &[
                "--database",
                "db",
                "--http",
                "7474",
                "--bolt-identity",
                "Neo4j/5.26.0",
            ][..],
            "--bolt-identity",
        ),
        (
            &[
                "--database",
                "db",
                "--http",
                "7474",
                "--ws-max-connections",
                "4",
            ][..],
            "--ws-max-connections",
        ),
        (
            &[
                "--database",
                "db",
                "--http",
                "7474",
                "--ws-allow-origin",
                "https://browser.graphapp.io",
            ][..],
            "--ws-allow-origin",
        ),
    ] {
        let err = parse_serve_args(&args(rest)).unwrap_err().to_string();
        assert!(err.contains(flag), "{rest:?}: {err}");
        assert!(err.contains("does not serve"), "{rest:?}: {err}");
    }
    // Beside the surface each flag configures, the same command parses.
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        "--bolt",
        "0",
        "--bolt-identity",
        "Neo4j/5.26.0",
    ]))
    .is_ok());
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        "--bolt",
        "0",
        "--ws-max-connections",
        "4",
    ]))
    .is_ok());
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        "--http",
        "7474",
        "--tx-idle-timeout-secs",
        "5",
    ]))
    .is_ok());
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        "--http",
        "7474",
        "--header-deadline-secs",
        "5",
    ]))
    .is_ok());
    // `--host` and `--max-connections` are command-posture knobs, not one
    // surface's story: tolerated on a bolt-only serve. `--auth` applies to
    // every listener, so it stays accepted there too.
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        "--bolt",
        "0",
        "--host",
        "0.0.0.0",
        "--max-connections",
        "4",
        "--auth",
        "a:b",
    ]))
    .is_ok());
}

#[cfg(all(feature = "mcp", feature = "http"))]
#[test]
fn parse_rejects_http_flags_on_an_mcp_listener_only_serve() {
    // The same policy reads the per-surface fact, not "is this run
    // stdio-only": an `--mcp PORT` listener does not serve HTTP, so an HTTP
    // flag is unbound there too.
    let err = parse_serve_args(&args(&[
        "--database",
        "db",
        "--mcp",
        "8000",
        "--max-open-transactions",
        "8",
    ]))
    .unwrap_err()
    .to_string();
    assert!(err.contains("--max-open-transactions"), "{err}");
    assert!(err.contains("--http"), "{err}");
    assert!(parse_serve_args(&args(&[
        "--database",
        "db",
        "--mcp",
        "8000",
        "--http",
        "7474",
        "--max-open-transactions",
        "8",
    ]))
    .is_ok());
}

#[cfg(all(feature = "mcp", feature = "http"))]
#[test]
fn parse_rejects_the_tx_limits_on_a_stdio_only_serve() {
    // ADR-0435 §4's knobs configure the HTTP listener's transaction
    // registry; a stdio-only run has no HTTP surface, so each is a loud
    // usage error rather than a silently discarded flag. Beside the HTTP
    // listener the same flag applies as usual.
    for (flag, value) in [
        ("--tx-idle-timeout-secs", "5"),
        ("--max-open-transactions", "8"),
    ] {
        let err = parse_serve_args(&args(&["--database", "db", "--mcp", "stdio", flag, value]))
            .unwrap_err()
            .to_string();
        assert!(err.contains(flag), "{flag}: {err}");
        assert!(
            parse_serve_args(&args(&[
                "--database",
                "db",
                "--mcp",
                "stdio",
                "--http",
                "7474",
                flag,
                value,
            ]))
            .is_ok(),
            "{flag} applies beside the HTTP listener"
        );
    }
}

#[cfg(feature = "mcp")]
#[test]
fn listener_flag_spellings_spell_the_compiled_surfaces() {
    // The listener spellings pinned as literals per compiled bundle
    // (round 4, F-4-4/F-4-5 — the projection itself is a one-line map;
    // what this test guards is the concrete wording the inert-flag
    // messages render): a change to a surface row's `listener_spelling`,
    // order, or set fails here instead of silently re-rendering the
    // messages. The test and the projection gate together with the
    // messages' policy (round 3, F-3-7).
    let spellings = listener_flag_spellings();
    #[cfg(feature = "http")]
    assert!(spellings.contains(&"--http PORT"), "{spellings:?}");
    assert!(spellings.contains(&"--mcp PORT"), "{spellings:?}");
    #[cfg(feature = "bolt")]
    assert!(spellings.contains(&"--bolt"), "{spellings:?}");
    // The whole enumeration on a full bundle: exactly these, in
    // SURFACE_FLAGS' order.
    #[cfg(all(feature = "http", feature = "mcp", feature = "bolt"))]
    assert_eq!(spellings, ["--http PORT", "--mcp PORT", "--bolt"]);
}
