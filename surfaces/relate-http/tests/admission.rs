//! Real-socket tests for the ADR-0451 admission protections: the absolute
//! header and body deadlines (a trickled phase is dropped at the deadline,
//! not reset by a fixed per-read timeout) and the non-zero connection cap
//! (over-cap connections are closed; a legitimate client still uses a free
//! slot at the cap).
//!
//! The server threads run until the test process exits (`serve` diverges by
//! design); every listener binds an ephemeral loopback port.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use relate::Database;
use relate_auth::AuthConfig;
use relate_http::{serve, serve_handler, HttpConfig};
use relate_serve::{
    DEFAULT_BODY_DEADLINE, DEFAULT_HEADER_DEADLINE, DEFAULT_MAX_CONNECTIONS, MAX_ADMISSION_DEADLINE,
};
use relate_test_support::loopback::{
    assert_dropped_while_trickling, exchange, request, LOOPBACK_HOST,
};

const DEADLINE: Duration = Duration::from_millis(300);

/// A server on an ephemeral port with the given config.
fn start(config: HttpConfig) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        serve(
            Arc::new(Database::open(":memory:").unwrap()),
            listener,
            config,
        )
        .expect("serve starts")
    });
    port
}

fn config() -> HttpConfig {
    let mut config = HttpConfig::new(AuthConfig::None);
    config.header_deadline = DEADLINE;
    // Deliberately far longer than either deadline: without the absolute
    // deadlines a trickle that never stalls a full read window would hold
    // the connection for this long.
    config.read_timeout = Duration::from_secs(5);
    config
}

/// One complete discovery request. `GET /` is public, so the normal-client
/// tests need no credentials or query.
fn discovery() -> String {
    request("GET", "/", LOOPBACK_HOST, &[("connection", "close")])
}

/// The request head for the body-trickle probe: a declared 100 KiB body
/// that the client then sends one byte at a time.
const BODY_PROBE_HEAD: &[u8] =
    b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 100000\r\n\r\n";

#[test]
fn a_trickled_header_block_is_dropped_at_the_absolute_deadline() {
    let port = start(config());
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // A header block that never terminates: the request line and one
    // header, then the helper's one byte per 50 ms.
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Trickle: ")
        .unwrap();
    let elapsed = assert_dropped_while_trickling(&mut stream, Duration::from_secs(2));
    assert!(
        elapsed >= Duration::from_millis(200),
        "the connection dropped before the deadline ({DEADLINE:?}): {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the connection outlived the deadline: {elapsed:?}"
    );
}

#[test]
fn a_trickled_body_is_dropped_at_the_absolute_body_deadline() {
    let mut config = config();
    config.body_deadline = DEADLINE;
    let port = start(config);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // A complete head declaring 100 KiB, then a body that never arrives
    // faster than the helper's trickle.
    stream.write_all(BODY_PROBE_HEAD).unwrap();
    let elapsed = assert_dropped_while_trickling(&mut stream, Duration::from_secs(2));
    assert!(
        elapsed >= Duration::from_millis(200),
        "the connection dropped before the body deadline ({DEADLINE:?}): {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the connection outlived the body deadline: {elapsed:?}"
    );
}

#[test]
fn a_rejected_requests_post_body_drain_is_absolutely_bounded() {
    // The drain after a pre-body rejection is also a peer-controlled body
    // read: its budget is absolute (REJECT_DRAIN_TIMEOUT), so a trickled
    // declared body cannot extend it one read window at a time.
    let mut config = HttpConfig::new(AuthConfig::basic("neo4j", "s3cret"));
    config.header_deadline = Duration::from_secs(10);
    config.read_timeout = Duration::from_secs(5);
    let port = start(config);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // No Authorization header: the gate rejects before the body, then
    // drains the declared body for at most REJECT_DRAIN_TIMEOUT.
    stream.write_all(BODY_PROBE_HEAD).unwrap();
    let elapsed = assert_dropped_while_trickling(&mut stream, Duration::from_secs(4));
    assert!(
        elapsed < Duration::from_secs(4),
        "the rejection drain outlived its absolute budget: {elapsed:?}"
    );
}

#[test]
fn a_normal_client_is_served_under_the_deadline() {
    let port = start(config());
    let (status, body) = exchange(port, discovery().as_bytes());
    assert_eq!(status, 200, "{body:?}");
}

#[test]
fn the_cap_admits_a_legitimate_client_and_refuses_the_next_connection() {
    let mut config = config();
    config.max_connections = 4;
    // Long enough that the idle holders stay admitted while the test
    // probes the cap.
    config.header_deadline = Duration::from_secs(10);
    let port = start(config);

    // Three sockets that connect and send nothing: one worker each.
    let idle: Vec<TcpStream> = (0..3)
        .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
        .collect();
    // Wait until their permits are taken (the accept loop hands them out
    // before the handler blocks on the header read).
    thread::sleep(Duration::from_millis(100));

    // The fourth slot serves a legitimate client at the cap.
    let (status, body) = exchange(port, discovery().as_bytes());
    assert_eq!(status, 200, "{body:?}");

    // More idle connections than free slots: the accept loop admits up to
    // the cap and closes the rest. Opening an excess makes the "cap is
    // full" state independent of how quickly each earlier accept landed.
    let excess: Vec<TcpStream> = (0..8)
        .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
        .collect();
    thread::sleep(Duration::from_millis(200));
    let mut over = TcpStream::connect(("127.0.0.1", port)).unwrap();
    over.write_all(discovery().as_bytes()).unwrap();
    over.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut response = Vec::new();
    match over.read_to_end(&mut response) {
        Ok(_) => {}
        // The server may close with an RST (nothing was read from the
        // socket), which the client sees as a reset rather than a clean
        // EOF. Both are the accept-then-close policy.
        Err(e) if e.kind() == ErrorKind::ConnectionReset => {}
        Err(e) => panic!("the over-cap probe failed: {e}"),
    }
    assert!(
        response.is_empty(),
        "an over-cap connection was served: {response:?}"
    );

    // Freeing slots restores service: a legitimate client is served again.
    drop(idle);
    drop(excess);
    let restored = (0..40).find_map(|_| {
        let (status, _) = exchange(port, discovery().as_bytes());
        if status == 200 {
            Some(())
        } else {
            thread::sleep(Duration::from_millis(50));
            None
        }
    });
    assert!(
        restored.is_some(),
        "the cap never released its slots to a legitimate client"
    );
}

#[test]
fn an_out_of_range_deadline_is_rejected_at_the_library_boundary() {
    // The CLI resolver bounds the flag; a directly constructed config is
    // caught where it enters the library, so the per-connection Instant
    // arithmetic never sees it.
    let over = MAX_ADMISSION_DEADLINE + Duration::from_secs(1);
    let mut config = HttpConfig::new(AuthConfig::None);
    config.header_deadline = over;
    assert!(config.validate().is_err());
    config.header_deadline = DEFAULT_HEADER_DEADLINE;
    config.body_deadline = over;
    let err = config.validate().unwrap_err();
    assert!(err.contains("body"), "{err}");

    // `serve` (the accept-loop entry) returns the typed error instead of
    // ending the embedder's process.
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let err = serve(
        Arc::new(Database::open(":memory:").unwrap()),
        listener,
        config_with_body_deadline(over),
    )
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput, "{err}");

    // `serve_handler` is the plaintext entry: it fails before reading.
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let err = serve_handler(&Database::open(":memory:").unwrap(), stream, &config).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput, "{err}");
    drop(client);
}

fn config_with_body_deadline(body_deadline: Duration) -> HttpConfig {
    let mut config = HttpConfig::new(AuthConfig::None);
    config.body_deadline = body_deadline;
    config
}

#[test]
fn the_defaults_are_bounded() {
    let config = HttpConfig::new(AuthConfig::None);
    assert_eq!(config.header_deadline, DEFAULT_HEADER_DEADLINE);
    assert!(config.header_deadline > Duration::ZERO);
    assert_eq!(config.body_deadline, DEFAULT_BODY_DEADLINE);
    assert!(config.body_deadline > Duration::ZERO);
    assert_eq!(config.max_connections, DEFAULT_MAX_CONNECTIONS);
    assert!(config.max_connections > 0);
}
