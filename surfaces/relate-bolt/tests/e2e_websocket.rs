//! End-to-end WebSocket tests (ADR-0266) against the real `relate-bolt`
//! binary: `ws://` and `wss://` Bolt exchanges, raw-TCP Bolt multiplexed on
//! the same port of the same process, `--no-websocket`, the Origin policy,
//! the plain-GET 400 path, and HELLO auth failure over the WebSocket
//! transport.
//!
//! The client side is blocking tungstenite over std sockets; `wss://`
//! terminates TLS with a blocking rustls `StreamOwned`, so no tungstenite TLS
//! feature is needed. Both crates are direct dependencies of the crate
//! (ADR-0233 — no tokio wrappers anywhere in the stack).

mod common;

use common::packstream::{decode, Pack};
use common::upgrade_request;
use common::{handshake, is_success, read_message, send, start_server, MAGIC, VERSION_4_4};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::CloseFrame;
use tungstenite::{Message, WebSocket};

/// The `user_agent` every HELLO in this file carries (Bolt 4.4 requires
/// one, ADR-0313).
const USER_AGENT: &str = "e2e-ws";

/// A blocking Bolt client over a WebSocket: outbound Bolt bytes go as binary
/// messages, inbound binary payloads concatenate into one byte stream (frame
/// and message boundaries carry no Bolt meaning, ADR-0266).
struct BoltWs<S: Read + Write> {
    ws: WebSocket<S>,
    buf: Vec<u8>,
}

impl<S: Read + Write> BoltWs<S> {
    /// Send raw Bolt bytes as one binary WebSocket message.
    fn send_bolt(&mut self, bytes: &[u8]) {
        self.ws
            .send(Message::Binary(bytes.to_vec().into()))
            .unwrap();
    }

    fn fill(&mut self, n: usize) {
        while self.buf.len() < n {
            match self.ws.read().unwrap() {
                Message::Binary(b) => self.buf.extend_from_slice(&b),
                other => panic!("unexpected WebSocket message from server: {other:?}"),
            }
        }
    }

    fn take(&mut self, n: usize) -> Vec<u8> {
        self.fill(n);
        self.buf.drain(..n).collect()
    }
}

/// Outbound writes become one binary WebSocket message each: the canonical
/// `common::send` writes one chunk-framed Bolt message per call, so the
/// transport-generic helpers drive this transport unchanged.
impl<S: Read + Write> Write for BoltWs<S> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.send_bolt(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The inbound WebSocket binary messages are one Bolt byte stream: `Read`
/// yields them in order, so the canonical chunk reader
/// (`common::read_message`) serves this transport too — WebSocket frame and
/// message boundaries carry no Bolt meaning (ADR-0266).
impl<S: Read + Write> Read for BoltWs<S> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        self.fill(1);
        let n = out.len().min(self.buf.len());
        out[..n].copy_from_slice(&self.buf[..n]);
        self.buf.drain(..n);
        Ok(n)
    }
}

/// Negotiate Bolt 4.4 with the handshake bytes in one binary WebSocket
/// message; the server's 4-byte version selection arrives as one message too.
fn ws_handshake<S: Read + Write>(client: &mut BoltWs<S>) {
    let mut offer = Vec::with_capacity(20);
    offer.extend_from_slice(&MAGIC);
    offer.extend_from_slice(&VERSION_4_4);
    client.send_bolt(&offer);
    assert_eq!(
        client.take(4),
        [0, 0, 4, 4],
        "server must offer Bolt 4.4 over WebSocket"
    );
}

/// Connect `ws://` with an optional Origin header; `Err` carries the
/// upgrade failure so callers can retry.
fn try_connect_ws(addr: &str, origin: Option<&str>) -> Result<BoltWs<TcpStream>, String> {
    let tcp = TcpStream::connect(addr).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    tcp.set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut request = "ws://localhost/".into_client_request().unwrap();
    if let Some(origin) = origin {
        request.headers_mut().insert(
            tungstenite::http::header::ORIGIN,
            tungstenite::http::HeaderValue::from_str(origin).unwrap(),
        );
    }
    let (ws, response) = tungstenite::client::client(request, tcp).map_err(|e| e.to_string())?;
    assert_eq!(
        response.status(),
        tungstenite::http::StatusCode::SWITCHING_PROTOCOLS
    );
    Ok(BoltWs {
        ws,
        buf: Vec::new(),
    })
}

/// A raw TCP client: connect + a 10 s read deadline (the e2e default).
fn connect_tcp(addr: &str) -> TcpStream {
    let client = TcpStream::connect(addr).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    client
}

/// Connect `ws://` with an optional Origin header and assert the 101.
fn connect_ws(addr: &str, origin: Option<&str>) -> BoltWs<TcpStream> {
    try_connect_ws(addr, origin).expect("ws:// upgrade failed")
}

/// HELLO (no credentials), RUN "RETURN 1 AS n", PULL; asserts the RECORD
/// carries `1`. Transport-agnostic: each test runs it after its own
/// transport's handshake (ws_handshake / handshake).
fn return_one<S: Read + Write>(client: &mut S) {
    send_hello(client);
    send(
        client,
        0x10,
        vec![Pack::String("RETURN 1 AS n".into()), Pack::Map(vec![])],
    );
    assert!(is_success(&read_message(client)), "RUN must succeed");
    send(
        client,
        0x3F,
        vec![Pack::Map(vec![(Pack::String("n".into()), Pack::Int(-1))])],
    );
    assert_eq!(
        decode(&read_message(client)).unwrap(),
        Pack::Struct {
            tag: 0x71,
            fields: vec![Pack::List(vec![Pack::Int(1)])],
        },
        "PULL must yield RECORD 1"
    );
    assert!(
        is_success(&read_message(client)),
        "PULL summary must succeed"
    );
}

/// ws://: the WebSocket handshake, then the shared HELLO/RUN/PULL dance.
fn return_one_over_ws<S: Read + Write>(client: &mut BoltWs<S>) {
    ws_handshake(client);
    return_one(client);
}

/// Extract the `code` field of a FAILURE message.
fn failure_code(msg: &[u8]) -> Option<String> {
    let Ok(Pack::Struct { tag: 0x7F, fields }) = decode(msg) else {
        return None;
    };
    let meta = fields.into_iter().next()?;
    meta.get("code")?.as_str().map(str::to_string)
}

/// The HELLO extra map every HELLO in this file sends: the required
/// `user_agent` (ADR-0313, [`USER_AGENT`]) plus `entries` (the basic-auth
/// fields).
fn hello_map(entries: Vec<(Pack, Pack)>) -> Pack {
    let mut map = vec![(
        Pack::String("user_agent".into()),
        Pack::String(USER_AGENT.into()),
    )];
    map.extend(entries);
    Pack::Map(map)
}

/// Send a plain HELLO (the `user_agent` only) and assert the SUCCESS.
fn send_hello<S: Read + Write>(client: &mut S) {
    send(client, 0x01, vec![hello_map(vec![])]);
    assert!(is_success(&read_message(client)), "HELLO must succeed");
}

/// Send a basic-auth HELLO (tag 0x01).
fn send_basic_hello<S: Read + Write>(client: &mut S, principal: &str, credentials: &str) {
    send(
        client,
        0x01,
        vec![hello_map(vec![
            (Pack::String("scheme".into()), Pack::String("basic".into())),
            (
                Pack::String("principal".into()),
                Pack::String(principal.into()),
            ),
            (
                Pack::String("credentials".into()),
                Pack::String(credentials.into()),
            ),
        ])],
    );
}

/// Assert a FAILURE message carries the `Neo.ClientError.Security.Unauthorized`
/// code.
fn assert_unauthorized(msg: &[u8]) {
    assert_eq!(
        failure_code(msg).as_deref(),
        Some("Neo.ClientError.Security.Unauthorized"),
        "wrong credentials over ws:// must fail with Unauthorized: {msg:?}"
    );
}

/// Raw-TCP Bolt: connect, the raw handshake, then the shared HELLO/RUN/PULL
/// dance.
fn return_one_over_raw_tcp(addr: &str) {
    let mut client = connect_tcp(addr);
    handshake(&mut client);
    return_one(&mut client);
}

/// Read one raw HTTP response head (through the blank line).
fn read_response_head(client: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        client.read_exact(&mut byte).unwrap();
        buf.push(byte[0]);
        assert!(buf.len() <= 8192, "response head unbounded");
    }
    String::from_utf8(buf).unwrap()
}

/// Generate a self-signed `localhost` certificate: (cert PEM, key PEM, cert
/// DER for the client's root store).
fn self_signed_localhost() -> (String, String, rustls::pki_types::CertificateDer<'static>) {
    let key = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let cert = params.self_signed(&key).unwrap();
    (cert.pem(), key.serialize_pem(), cert.der().clone())
}

/// Connect `wss://` over a blocking rustls stream that trusts `cert_der`.
fn connect_wss(
    addr: &str,
    cert_der: rustls::pki_types::CertificateDer<'static>,
) -> BoltWs<rustls::StreamOwned<rustls::ClientConnection, TcpStream>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der).unwrap();
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from("localhost")
        .unwrap()
        .to_owned();
    let conn = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
    let tcp = TcpStream::connect(addr).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    tcp.set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let tls = rustls::StreamOwned::new(conn, tcp);
    let request = "wss://localhost/".into_client_request().unwrap();
    let (ws, response) = tungstenite::client::client(request, tls).expect("wss:// upgrade failed");
    assert_eq!(
        response.status(),
        tungstenite::http::StatusCode::SWITCHING_PROTOCOLS
    );
    BoltWs {
        ws,
        buf: Vec::new(),
    }
}

#[test]
fn ws_serves_hello_run_pull() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_ws(&server.addr, None);
    return_one_over_ws(&mut client);
}

#[test]
fn raw_tcp_bolt_multiplexes_with_ws_on_one_port() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    // Same port, same process: the bind multiplexes ws:// and raw-TCP Bolt.
    let mut client = connect_ws(&server.addr, None);
    return_one_over_ws(&mut client);
    return_one_over_raw_tcp(&server.addr);
}

#[test]
fn wss_serves_bolt_over_tls() {
    let dir = tempfile::tempdir().unwrap();
    let (cert_pem, key_pem, cert_der) = self_signed_localhost();
    let cert_path = dir.path().join("cert.pem");
    let key_path = dir.path().join("key.pem");
    std::fs::write(&cert_path, cert_pem).unwrap();
    std::fs::write(&key_path, key_pem).unwrap();
    let server = start_server(&[
        "--uri",
        ":memory:",
        "--auth",
        "none",
        "--tls-cert",
        cert_path.to_str().unwrap(),
        "--tls-key",
        key_path.to_str().unwrap(),
    ]);
    let mut client = connect_wss(&server.addr, cert_der);
    return_one_over_ws(&mut client);
}

#[test]
fn no_websocket_closes_the_upgrade_without_a_101() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none", "--no-websocket"]);
    let mut client = connect_tcp(&server.addr);
    client
        .write_all(upgrade_request("dGhlIHNhbXBsZSBub25jZQ==", "").as_bytes())
        .unwrap();
    // The guard fails: the connection closes like any unrecognized first
    // byte — no 101, no HTTP response at all.
    assert_no_reply(&mut client, "--no-websocket");
}

#[test]
fn origin_policy_over_the_wire() {
    // --auth none: a cross-site Origin is rejected with 403 and no 101.
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_tcp(&server.addr);
    client
        .write_all(
            upgrade_request(
                "dGhlIHNhbXBsZSBub25jZQ==",
                "Origin: http://evil.example\r\n",
            )
            .as_bytes(),
        )
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(
        head.starts_with("HTTP/1.1 403 Forbidden"),
        "expected 403 for a cross-site Origin, got: {head:?}"
    );
    drop(server);

    // --auth none: a loopback Origin gets 101 and Bolt works.
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_ws(&server.addr, Some("http://localhost:3000"));
    return_one_over_ws(&mut client);
    drop(server);

    // --auth none with an explicit allowlist: that Origin gets 101.
    let server = start_server(&[
        "--uri",
        ":memory:",
        "--auth",
        "none",
        "--ws-allow-origin",
        "https://browser.graphapp.io",
    ]);
    let mut client = connect_ws(&server.addr, Some("https://browser.graphapp.io"));
    return_one_over_ws(&mut client);
    drop(server);

    // auth on (ADR-0459): the hosted Neo4j Browser and Console Origins are
    // admitted by the default; any other Origin is refused at the upgrade.
    let server = start_server(&["--uri", ":memory:", "--auth", "neo4j:secret"]);
    let _browser = connect_ws(&server.addr, Some("https://browser.neo4j.io"));
    let _console = connect_ws(&server.addr, Some("https://console.neo4j.io"));
    assert!(
        try_connect_ws(&server.addr, Some("https://evil.example")).is_err(),
        "a foreign Origin must be refused with auth on"
    );
}

#[test]
fn plain_get_without_upgrade_gets_400() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_tcp(&server.addr);
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(
        head.starts_with("HTTP/1.1 400 Bad Request"),
        "expected 400 for a plain GET, got: {head:?}"
    );
}

#[test]
fn ws_hello_wrong_credentials_is_unauthorized() {
    let server = start_server(&["--uri", ":memory:", "--auth", "neo4j:secret"]);
    let mut client = connect_ws(&server.addr, None);
    ws_handshake(&mut client);
    send_basic_hello(&mut client, "neo4j", "wrong");
    assert_unauthorized(&read_message(&mut client));
}

/// Read WebSocket messages until the server's Close frame arrives; any EOF
/// or error before it is a failure.
fn read_until_close<S: Read + Write>(client: &mut BoltWs<S>) -> Option<CloseFrame> {
    loop {
        match client.ws.read() {
            Ok(Message::Close(frame)) => return frame,
            Ok(_) => {}
            Err(e) => panic!("connection ended before the server's Close frame: {e}"),
        }
    }
}

/// `connect_ws`, retrying while the upgrade fails: at `--max-connections 1`
/// a transient permit holder (the harness's bind probe, a just-dropped
/// connection) can briefly occupy the only slot.
fn connect_ws_retry(addr: &str) -> BoltWs<TcpStream> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match try_connect_ws(addr, None) {
            Ok(client) => return client,
            Err(e) => {
                assert!(
                    Instant::now() < deadline,
                    "ws:// upgrade never succeeded: {e}"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Assert a connection that must get no reply is closed (EOF) or errors
/// without the server sending any bytes.
fn assert_no_reply(client: &mut TcpStream, what: &str) {
    let mut buf = [0u8; 64];
    match client.read(&mut buf) {
        Ok(0) | Err(_) => {}
        Ok(n) => panic!(
            "{what} must get no reply, got {n} bytes: {:?}",
            String::from_utf8_lossy(&buf[..n])
        ),
    }
}

#[test]
fn ws_connection_cap_rejects_then_recovers() {
    // ADR-0110: one permit per accepted connection; at the cap a new
    // connection is accepted and closed with no reply (no 101).
    let server = start_server(&[
        "--uri",
        ":memory:",
        "--auth",
        "none",
        "--max-connections",
        "1",
    ]);
    // The first connection holds the only permit: complete the WS upgrade
    // but send no Bolt magic yet — the permit is held from accept, not from
    // the Bolt handshake.
    let first = connect_ws_retry(&server.addr);

    let mut second = connect_tcp(&server.addr);
    second
        .write_all(upgrade_request("dGhlIHNhbXBsZSBub25jZQ==", "").as_bytes())
        .unwrap();
    assert_no_reply(&mut second, "a capped connection (no 101, no reply)");
    drop(second);
    drop(first);

    // Once the permit is released a new ws:// connection completes.
    let mut client = connect_ws_retry(&server.addr);
    return_one_over_ws(&mut client);
}

#[test]
fn ws_upgrade_stall_hits_the_single_handshake_deadline() {
    // ADR-0141: one accept-time deadline covers every negotiation phase; a
    // stalled WS upgrade is closed at ~1 s (not one budget per phase).
    let server = start_server(&[
        "--uri",
        ":memory:",
        "--auth",
        "none",
        "--handshake-timeout-secs",
        "1",
    ]);
    // This test needs a shorter deadline than the e2e default.
    let mut client = connect_tcp(&server.addr);
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    // A partial upgrade request: the server waits for the rest of the head.
    client.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    let sent = Instant::now();
    assert_no_reply(&mut client, "a stalled upgrade");
    let elapsed = sent.elapsed();
    assert!(
        elapsed <= Duration::from_secs(2),
        "the single accept-time deadline must close the connection at ~1 s, took {elapsed:?}"
    );

    // The phase-specific log line (`bolt: websocket upgrade timeout;
    // closing connection`, websocket.rs) fires at the same close — the lib
    // prints it unconditionally on that path. The in-process harness
    // (LABS-381, ADR-0368) cannot capture the serve thread's stderr — the old spawn
    // asserted the line there — so the observable in-process pin is the
    // close itself, which the `assert_no_reply` above and this deadline
    // bound carry.
}

#[test]
fn ws_route_advertises_the_dialed_address() {
    // ADR-0269/0140 over ws://: ROUTE answers a single-server self routing
    // table whose every role advertises the routing-context address the
    // client dialed, echoing the requested db.
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let port = server.addr.rsplit(':').next().unwrap();
    let dialed = format!("localhost:{port}");
    let mut client = connect_ws(&server.addr, None);
    ws_handshake(&mut client);
    send_hello(&mut client);
    send(
        &mut client,
        0x66,
        vec![
            Pack::Map(vec![(
                Pack::String("address".into()),
                Pack::String(dialed.clone()),
            )]),
            Pack::List(vec![]),
            Pack::Map(vec![(
                Pack::String("db".into()),
                Pack::String("system".into()),
            )]),
        ],
    );
    let msg = read_message(&mut client);
    let Ok(Pack::Struct { tag: 0x70, fields }) = decode(&msg) else {
        panic!("ROUTE over WebSocket must succeed: {msg:?}");
    };
    let rt = fields
        .into_iter()
        .next()
        .and_then(|m| m.get("rt").cloned())
        .expect("ROUTE SUCCESS must carry an rt map");
    assert_eq!(
        rt.get("db").and_then(Pack::as_str),
        Some("system"),
        "rt.db echoes the requested db"
    );
    let Some(Pack::List(servers)) = rt.get("servers") else {
        panic!("rt must carry a servers list: {rt:?}");
    };
    let expected = Pack::List(vec![Pack::String(dialed.clone())]);
    for role in ["ROUTE", "READ", "WRITE"] {
        let entry = servers
            .iter()
            .find(|s| s.get("role").and_then(Pack::as_str) == Some(role))
            .unwrap_or_else(|| panic!("servers must contain a {role} entry: {servers:?}"));
        assert_eq!(
            entry.get("addresses"),
            Some(&expected),
            "the {role} entry must advertise the dialed address {dialed}"
        );
    }
}

#[test]
fn ws_failed_hello_is_delayed() {
    // ADR-0139: a failed HELLO is answered with Unauthorized no sooner than
    // the fixed 1000 ms delay, on the ws:// transport too.
    let server = start_server(&["--uri", ":memory:", "--auth", "relate:test-pw"]);
    let mut client = connect_ws(&server.addr, None);
    ws_handshake(&mut client);
    let sent = Instant::now();
    send_basic_hello(&mut client, "relate", "wrong");
    let msg = read_message(&mut client);
    let elapsed = sent.elapsed();
    assert_unauthorized(&msg);
    assert!(
        elapsed >= Duration::from_millis(900),
        "the failed-HELLO delay is 1000 ms; the FAILURE arrived after {elapsed:?}"
    );
    assert!(
        elapsed <= Duration::from_secs(8),
        "the failed-HELLO FAILURE must not hang, took {elapsed:?}"
    );
}

#[test]
fn ws_client_close_gets_a_close_frame_back() {
    // A client-initiated Close(1000) is answered with a server Close frame
    // before EOF (WebSocket close handshake, ADR-0266).
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_ws(&server.addr, None);
    ws_handshake(&mut client);
    send_hello(&mut client);
    client
        .ws
        .send(Message::Close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "done".into(),
        })))
        .unwrap();
    let frame = read_until_close(&mut client);
    assert_eq!(
        frame.map(|f| f.code),
        Some(CloseCode::Normal),
        "the server must echo a normal Close frame"
    );
}

#[test]
fn ws_goodbye_gets_a_close_frame_before_eof() {
    // GOODBYE ends the session; over ws:// the server sends a WebSocket
    // Close frame before closing the TCP connection (ADR-0266).
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_ws(&server.addr, None);
    return_one_over_ws(&mut client);
    send(&mut client, 0x02, vec![]);
    let frame = read_until_close(&mut client);
    assert_eq!(
        frame.map(|f| f.code),
        Some(CloseCode::Normal),
        "GOODBYE must end in a normal server Close frame"
    );
}
