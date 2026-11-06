//! End-to-end auth integration test (ADR-0126): the real listener (the
//! crate's `serve`, in-process since LABS-381, ADR-0368 retired the standalone
//! binary), real sockets, the generated-token persistence path included.

mod common;

use clap::Parser;
use common::packstream::{decode, Pack};
use common::{handshake, is_success, read_message, send, start_server};
use relate_bolt::cli_args::Args;
use relate_bolt::serve;
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// The generated token from the `<store>.auth-token` file the
/// generated-token path persists — the same token the startup banner the
/// removed binary printed carried (`auth: generated token for user
/// relate: <hex>`). The in-process harness cannot capture the banner
/// (the server's stderr is the test process's stderr), so the persisted
/// file is the observable surface — and it requires `RELATE_AUTH` to
/// be unset in the parent env: the in-process server reads the real
/// environment, and the spawn's old `env_remove` isolation cannot be
/// replayed without racing the parallel serve threads.
/// The trimmed line the token file stores, `relate:<token>` — the
/// one reader both assertions share (the format and the token).
fn stored_token_line(db_path: &str) -> String {
    let token_file = relate_auth::token_path(db_path)
        .expect("the temp store path is a local URI")
        .expect("a persistent store has a token path");
    std::fs::read_to_string(&token_file)
        .expect("generated token persisted")
        .trim()
        .to_string()
}

fn persisted_token(db_path: &str) -> String {
    stored_token_line(db_path)
        .strip_prefix("relate:")
        .expect("the file stores `relate:<token>`")
        .to_string()
}

fn hello(client: &mut TcpStream, pairs: Vec<(Pack, Pack)>) -> Vec<u8> {
    send(client, 0x01, vec![Pack::Map(pairs)]);
    read_message(client)
}

fn is_failure(msg: &[u8]) -> bool {
    matches!(decode(msg), Ok(Pack::Struct { tag: 0x7F, .. }))
}

fn connect_and_hello(addr: &str, auth: Option<(&str, &str)>) -> Vec<u8> {
    let mut client = TcpStream::connect(addr).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    handshake(&mut client);
    let mut pairs = vec![(
        Pack::String("user_agent".into()),
        Pack::String("e2e-auth".into()),
    )];
    if let Some((user, password)) = auth {
        pairs.push((Pack::String("scheme".into()), Pack::String("basic".into())));
        pairs.push((Pack::String("principal".into()), Pack::String(user.into())));
        pairs.push((
            Pack::String("credentials".into()),
            Pack::String(password.into()),
        ));
    }
    let msg = hello(&mut client, pairs);
    if is_success(&msg) {
        // prove the session actually queries: RUN + PULL
        send(
            &mut client,
            0x10,
            vec![Pack::String("RETURN 1 AS n".into()), Pack::Map(vec![])],
        );
        let _ = read_message(&mut client);
        send(
            &mut client,
            0x3F,
            vec![Pack::Map(vec![(Pack::String("n".into()), Pack::Int(-1))])],
        );
        let _ = read_message(&mut client); // RECORD
        let last = read_message(&mut client); // SUCCESS
        assert!(is_success(&last), "PULL should succeed: {last:?}");
    }
    msg
}

#[test]
fn auth_end_to_end_generated_preset_none_and_persistence() {
    // 1. generated token: read it from the persisted token file, connect
    // with it
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("graph").to_str().unwrap().to_string();
    let server = start_server(&["--uri", &db_path]);
    let token = persisted_token(&db_path);
    assert_eq!(token.len(), 32, "128-bit hex token");

    let msg = connect_and_hello(&server.addr, Some(("relate", &token)));
    assert!(is_success(&msg), "generated token must authenticate");
    let msg = connect_and_hello(&server.addr, Some(("relate", "wrong")));
    assert!(is_failure(&msg), "wrong credentials must fail");
    let msg = connect_and_hello(&server.addr, None);
    assert!(is_failure(&msg), "no credentials must fail when auth is on");

    // 2. persistence: a second serve on the same directory reuses the
    // token. The second serve opens read-only — the in-process harness
    // cannot stop the first server's thread (its store stays open), and a
    // read-only open beside the still-open store is the designed
    // second-handle shape (ADR-0136); auth resolves and the token file is
    // read exactly as on a writable boot.
    let server = start_server(&["--uri", &db_path, "--read-only"]);
    assert_eq!(stored_token_line(&db_path), format!("relate:{token}"));
    assert_eq!(persisted_token(&db_path), token, "the token is reused");
    let msg = connect_and_hello(&server.addr, Some(("relate", &token)));
    assert!(is_success(&msg), "the same token works across a restart");

    // 3. explicit preset
    let server = start_server(&["--uri", ":memory:", "--auth", "neo4j:secret"]);
    let msg = connect_and_hello(&server.addr, Some(("neo4j", "secret")));
    assert!(is_success(&msg));
    let msg = connect_and_hello(&server.addr, Some(("neo4j", "nope")));
    assert!(is_failure(&msg));

    // 4. --auth none: the pre-ADR behavior, any/no credentials accepted
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let msg = connect_and_hello(&server.addr, Some(("anyone", "anything")));
    assert!(is_success(&msg));
    let msg = connect_and_hello(&server.addr, None);
    assert!(is_success(&msg));
}

#[test]
fn malformed_auth_flags_are_startup_errors() {
    // A present-but-malformed value is a loud startup error, never a
    // silent fallback to no-auth. The removed binary surfaced this as its
    // stderr and exit status; the in-process listener surfaces the same
    // resolution as `serve`'s error.
    for bad in ["nocolon", ":secret", "relate:"] {
        let args = Args::try_parse_from(["relate-bolt", "--uri", ":memory:", "--auth", bad])
            .expect("the grammar accepts the value; the resolution rejects it");
        let err = serve(&args).unwrap_err().to_string();
        assert!(err.contains("--auth:"), "{bad:?}: got {err}");
    }
}

#[test]
fn start_server_reports_startup_crash_stderr() {
    // LABS-144: a startup crash must fail fast with the server's error in
    // the panic — the pre-fix harness polled the dead port for the full 10s
    // and then panicked with "server never bound" and no evidence. The
    // in-process harness carries the serve thread's error where the spawn
    // carried the drained stderr.
    let start = Instant::now();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = start_server(&["--uri", ":memory:", "--auth", "nocolon"]);
    }))
    .expect_err("a startup crash must panic");
    let msg = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("<non-string panic>")
        .to_string();
    assert!(msg.contains("server never bound"), "{msg}");
    assert!(
        msg.contains("--auth: expected user:password or 'none'"),
        "the panic must carry the server's own error: {msg}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "a dead serve thread is not polled for the full timeout: {:?}",
        start.elapsed()
    );
}
