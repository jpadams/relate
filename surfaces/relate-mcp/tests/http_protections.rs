//! The MCP streamable-HTTP transport rides the shared HTTP/1.1 stack of
//! `relate-http`, so its protections apply here too. These tests drive
//! raw bytes over a real socket against `serve_http_on` and assert each
//! guard answers `400` instead of the old hand-rolled copy's behavior
//! (uncapped headers, a body cap fed from an unverified `Content-Length`
//! straight into a `vec!` allocation, a malformed `Content-Length` silently
//! parsed as 0, and `String::from_utf8_lossy` mangling) — and that the
//! shared version floor refuses a non-HTTP/1.1 request line on this surface
//! too.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use relate::Database;
use relate_auth::AuthConfig;
use relate_mcp::{serve_http_on, serve_shared, McpServer, McpServerConfig, ServeError};
use relate_serve::DEFAULT_MAX_BODY_BYTES;
use relate_serve::MAX_ADMISSION_DEADLINE;
use relate_test_support::loopback::{
    assert_dropped_while_trickling, exchange, post_with_version, request, LOOPBACK_HOST,
};

/// Start the transport on an ephemeral port in the background; it runs
/// until the test process ends.
fn start() -> u16 {
    start_with(AuthConfig::None)
}

/// [`start`] with a specific auth configuration.
fn start_with(auth: AuthConfig) -> u16 {
    start_with_config(McpServerConfig {
        auth,
        ..McpServerConfig::default()
    })
}

/// [`start`] with a specific server configuration.
fn start_with_config(config: McpServerConfig) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = McpServer::new(Database::open(":memory:").unwrap(), config);
    std::thread::spawn(move || serve_http_on(server, listener).expect("serve_http_on starts"));
    port
}

#[test]
fn http_1_0_is_refused_with_400_by_the_shared_version_floor() {
    let port = start();
    // The version floor lives in the shared gate, so this transport answers a
    // non-HTTP/1.1 request line exactly as the Query API does: a parsed 400
    // rejection, never a served request (ADR-0386 §8).
    let request = post_with_version(
        "HTTP/1.0",
        "/mcp",
        r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
    );
    let (status, body) = exchange(port, request.as_bytes());
    assert_eq!(status, 400, "{body:?}");
    let error: serde_json::Value = serde_json::from_str(&body).expect("the 400 is a JSON error");
    let message = error["error"]
        .as_str()
        .expect("the MCP error envelope carries an error message");
    assert!(
        message.contains("HTTP/1.0") && message.contains("HTTP/1.1"),
        "the refusal names the versions: {message}"
    );
}

#[test]
fn oversized_body_is_rejected_before_allocation() {
    let port = start();
    // One byte over the shared cap — only the header block is sent; a
    // reader that trusted Content-Length would try to allocate and block on
    // the body, so getting a prompt 400 proves the cap hit first.
    let request = format!(
        "POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        DEFAULT_MAX_BODY_BYTES + 1
    );
    let (status, response) = exchange(port, request.as_bytes());
    assert_eq!(status, 400, "{response:?}");
}

#[test]
fn oversized_headers_are_rejected() {
    let port = start();
    // A header line that pushes the header block past the 64 KiB cap — and
    // with no newline until past it, so the per-line read bound (not just
    // the total cap) is what fires.
    let mut request = b"POST /mcp HTTP/1.1\r\n".to_vec();
    request.extend_from_slice(b"X-Filler: ");
    request.extend(std::iter::repeat_n(b'x', 128 * 1024));
    request.extend_from_slice(b"\r\n\r\n");
    let (status, response) = exchange(port, &request);
    assert_eq!(status, 400, "{response:?}");
}

#[test]
fn malformed_content_length_is_rejected_not_zeroed() {
    let port = start();
    // The old copy parsed this with unwrap_or(0) and answered the empty
    // body; the shared reader rejects it as malformed.
    let request =
        b"POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: nope\r\n\r\n{}";
    let (status, response) = exchange(port, request);
    assert_eq!(status, 400, "{response:?}");
}

#[test]
fn non_utf8_body_is_rejected_not_mangled() {
    let port = start();
    // 0xFF 0xFE is never valid UTF-8; the old copy's from_utf8_lossy would
    // have silently mangled it into replacement characters.
    let request =
        b"POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n\xff\xfe";
    let (status, response) = exchange(port, request);
    assert_eq!(status, 400, "{response:?}");
}

#[test]
fn browser_guard_rejects_foreign_origins_and_non_json_posts() {
    let port = start();
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    let post = |extra_headers: &str| {
        format!(
            "POST /mcp HTTP/1.1\r\n{extra_headers}Content-Length: {}\r\n\r\n{body}",
            body.len()
        )
    };
    // A cross-origin form POST cannot set application/json: 415.
    let (status, response) = exchange(port, post("Content-Type: text/plain\r\n").as_bytes());
    assert_eq!(status, 415, "{response:?}");
    // A POST with no Content-Type at all: 415.
    let (status, response) = exchange(port, post("").as_bytes());
    assert_eq!(status, 415, "{response:?}");
    // A present foreign Origin: 403.
    let (status, response) = exchange(
        port,
        post("Content-Type: application/json\r\nOrigin: https://evil.example\r\n").as_bytes(),
    );
    assert_eq!(status, 403, "{response:?}");
    // A loopback Origin passes and the request is served.
    let (status, response) = exchange(
        port,
        post("Content-Type: application/json\r\nOrigin: http://127.0.0.1:3000\r\n").as_bytes(),
    );
    assert_eq!(status, 200, "{response:?}");
    assert!(response.contains("\"result\":{}"), "{response:?}");
}

#[test]
fn the_shared_guard_covers_unsafe_non_post_methods_on_the_agent_transport() {
    let port = start();
    // The MCP transport answers POST only, but the Origin guard is the
    // shared policy, not a per-route check: a foreign-origin DELETE is
    // refused before the transport's routing.
    let foreign = request(
        "DELETE",
        "/mcp",
        LOOPBACK_HOST,
        &[("origin", "https://evil.example")],
    );
    let (status, response) = exchange(port, foreign.as_bytes());
    assert_eq!(status, 403, "{response:?}");
    // A loopback Origin passes the guard and reaches the transport's own
    // POST-only 405.
    let allowed = request(
        "DELETE",
        "/mcp",
        LOOPBACK_HOST,
        &[("origin", "http://127.0.0.1:3000")],
    );
    let (status, response) = exchange(port, allowed.as_bytes());
    assert_eq!(status, 405, "{response:?}");
}

#[test]
fn unauthenticated_over_cap_post_is_401_before_the_body_is_read() {
    let port = start_with(AuthConfig::basic("neo4j", "s3cret"));
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // Headers only: an over-cap Content-Length and no body bytes at all. A
    // reader that buffered the body before the auth check would block here
    // (or answer 400 on the cap); the prompt 401 proves auth ran first.
    let request = format!(
        "POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        DEFAULT_MAX_BODY_BYTES + 1
    );
    stream.write_all(request.as_bytes()).unwrap();
    let started = std::time::Instant::now();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 401"), "{response:?}");
    assert!(response.contains("WWW-Authenticate"), "{response:?}");
    // The ADR-0139 failed-auth delay applies to the 401.
    assert!(
        started.elapsed() >= relate_http::http::FAILED_AUTH_DELAY,
        "401 without the failed-auth delay"
    );
}

#[test]
fn a_trickled_header_block_is_dropped_at_the_absolute_header_deadline() {
    // The MCP transport consumes the same shared gate as the Query API
    // (ADR-0451): the deadline arms every header read with the time
    // remaining, so one byte per 50 ms — always inside the 5 s per-read
    // timeout — cannot hold the worker past the 300 ms deadline.
    let deadline = Duration::from_millis(300);
    let port = start_with_config(McpServerConfig {
        header_deadline: deadline,
        read_timeout: Duration::from_secs(5),
        ..McpServerConfig::default()
    });
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nX-Trickle: ")
        .unwrap();
    let elapsed = assert_dropped_while_trickling(&mut stream, Duration::from_secs(2));
    assert!(
        elapsed >= Duration::from_millis(200),
        "the MCP drop came before the deadline ({deadline:?}): {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "a trickled header block held the MCP worker past 2 s: {elapsed:?}"
    );
}

#[test]
fn a_trickled_body_is_dropped_at_the_absolute_body_deadline() {
    // The body read consumes the same shared reader (ADR-0451): a declared
    // body that never arrives faster than the trickle is dropped at the
    // absolute body deadline, not held by the resettable per-read timeout.
    let deadline = Duration::from_millis(300);
    let port = start_with_config(McpServerConfig {
        body_deadline: deadline,
        read_timeout: Duration::from_secs(5),
        ..McpServerConfig::default()
    });
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(
            b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 100000\r\n\r\n",
        )
        .unwrap();
    let elapsed = assert_dropped_while_trickling(&mut stream, Duration::from_secs(2));
    assert!(
        elapsed >= Duration::from_millis(200),
        "the MCP body drop came before the deadline ({deadline:?}): {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "a trickled body held the MCP worker past 2 s: {elapsed:?}"
    );
}

#[test]
fn an_out_of_range_deadline_is_rejected_at_the_library_boundary() {
    // The CLI resolver bounds the flag; a directly constructed config is
    // caught where it enters the library.
    let config = McpServerConfig {
        body_deadline: MAX_ADMISSION_DEADLINE + Duration::from_secs(1),
        ..McpServerConfig::default()
    };
    assert!(config.validate().is_err());
    let err = serve_shared(Arc::new(Database::open(":memory:").unwrap()), config).unwrap_err();
    let ServeError::InvalidConfig(message) = &err else {
        panic!("expected an InvalidConfig error, got {err}");
    };
    assert!(message.contains("body deadline"), "{message}");
}

#[test]
fn serve_http_on_returns_the_error_for_an_out_of_range_config() {
    // The listener entry returns the typed error instead of ending the
    // embedder's process when a directly constructed config is invalid.
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let server = McpServer::new(
        Database::open(":memory:").unwrap(),
        McpServerConfig {
            header_deadline: MAX_ADMISSION_DEADLINE + Duration::from_secs(1),
            ..McpServerConfig::default()
        },
    );
    let err = serve_http_on(server, listener).unwrap_err();
    let ServeError::InvalidConfig(message) = &err else {
        panic!("expected an InvalidConfig error, got {err}");
    };
    assert!(message.contains("header deadline"), "{message}");
}
