use super::*;
use crate::packstream::Pack;
use crate::protocol::{read_message, ConnConfig, SocketTimeouts};
use crate::testutil::upgrade_request;
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
use tungstenite::client::IntoClientRequest;

const RFC6455_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
const RFC6455_ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

/// The read/write guard armed on every test socket: a wedged server fails
/// the test instead of hanging the suite.
const GUARD: Duration = Duration::from_secs(10);

fn guarded(stream: TcpStream) -> TcpStream {
    stream.set_read_timeout(Some(GUARD)).unwrap();
    stream.set_write_timeout(Some(GUARD)).unwrap();
    stream
}

/// A loopback socket pair: the server side, and the connected client side.
fn socket_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let client = TcpStream::connect(addr).unwrap();
    let (server, _) = listener.accept().unwrap();
    (guarded(server), guarded(client))
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

/// One masked client frame (FIN/opcode, mask key 0x11223344).
fn masked_frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![if fin { 0x80 | opcode } else { opcode }];
    let len = payload.len();
    if len >= 65536 {
        out.push(0xFF);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    } else if len >= 126 {
        out.push(0xFE);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x80 | len as u8);
    }
    let mask = [0x11, 0x22, 0x33, 0x44];
    out.extend_from_slice(&mask);
    out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

/// Server-side accept result used by the test threads.
type AcceptResult = std::io::Result<BoltWsStream<TcpStream>>;

/// Collect WebSocket messages until the server's Close frame arrives;
/// panics on a stream error (the socket read guard turns a missing close
/// into one — including the timeout that `eof_msg` described).
fn read_until_close(ws: &mut WebSocket<TcpStream>, eof_msg: &str) -> Option<CloseFrame> {
    loop {
        match ws.read() {
            Ok(Message::Close(frame)) => break frame,
            Ok(_) => continue,
            Err(e) => panic!("ws error (or {eof_msg}): {e}"),
        }
    }
}

/// Spawn a thread draining the adapter's read side until EOF or error (the
/// close handshake completing).
fn spawn_eof_reader(mut adapter: BoltWsStream<TcpStream>) -> std::thread::JoinHandle<()> {
    // The thread must not hang the test on a misbehaving peer: the socket
    // timeout turns a missing EOF into a read error, which ends the loop.
    adapter.set_read_timeout(Some(GUARD)).unwrap();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16];
        loop {
            match adapter.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => continue,
            }
        }
    })
}

/// Run `accept_websocket` on the server half of a socket pair and return
/// the client half plus the server thread.
fn accept_thread(config: WsConfig) -> (TcpStream, std::thread::JoinHandle<AcceptResult>) {
    let (server, client) = socket_pair();
    let task = std::thread::spawn(move || accept_websocket(server, &config, None));
    (client, task)
}

/// Complete a real handshake through the accept path with a tungstenite
/// client; returns the client WebSocket and the server-side adapter.
fn ws_pair(config: WsConfig) -> (WebSocket<TcpStream>, BoltWsStream<TcpStream>) {
    let (client, task) = accept_thread(config);
    let request = "ws://localhost/".into_client_request().unwrap();
    let (ws, _response) = tungstenite::client(request, client).expect("client handshake failed");
    let adapter = task
        .join()
        .expect("server thread panicked")
        .expect("server accept failed");
    (ws, adapter)
}

/// Run a full `serve_websocket` session (permit-holding included) on a
/// socket pair and complete the client handshake; the returned client keeps
/// the session open until it is dropped.
fn ws_session(config: WsConfig) -> (WebSocket<TcpStream>, std::thread::JoinHandle<()>) {
    let db = Arc::new(Database::open(":memory:").unwrap());
    let (server, client) = socket_pair();
    let task = std::thread::spawn(move || {
        // The test client drops without a close handshake, which the server
        // reports as a session error; the session is over either way.
        let _ = serve_websocket(server, &config, db, ConnConfig::default(), None);
    });
    let request = "ws://localhost/".into_client_request().unwrap();
    let (ws, _) = tungstenite::client(request, client).expect("client handshake failed");
    (ws, task)
}

#[test]
fn rfc6455_sample_key_produces_the_sample_accept() {
    let (mut client, task) = accept_thread(WsConfig::default());
    client
        .write_all(upgrade_request(RFC6455_KEY, "").as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(
        head.starts_with("HTTP/1.1 101"),
        "expected 101 Switching Protocols, got: {head:?}"
    );
    assert!(
        head.to_ascii_lowercase()
            .contains(&format!("sec-websocket-accept: {RFC6455_ACCEPT}").to_ascii_lowercase()),
        "missing/incorrect Sec-WebSocket-Accept in: {head:?}"
    );
    task.join().unwrap().expect("accept must succeed");
}

#[test]
fn bolt_message_split_across_three_binary_frames_reads_back_intact() {
    let (mut ws, mut adapter) = ws_pair(WsConfig::default());
    let hello = crate::packstream::struct_msg(0x01, vec![Pack::Map(vec![])]);
    assert!(hello.len() >= 3);
    let third = hello.len() / 3;
    for part in [
        &hello[..third],
        &hello[third..2 * third],
        &hello[2 * third..],
    ] {
        ws.send(Message::Binary(part.to_vec().into())).unwrap();
    }
    let payload = read_message(&mut adapter).unwrap();
    assert_eq!(
        crate::packstream::decode(&payload).unwrap(),
        Pack::Struct {
            tag: 0x01,
            fields: vec![Pack::Map(vec![])],
        }
    );
}

#[test]
fn two_bolt_messages_in_one_frame_read_back_as_two() {
    let (mut ws, mut adapter) = ws_pair(WsConfig::default());
    let hello = crate::packstream::struct_msg(0x01, vec![Pack::Map(vec![])]);
    let goodbye = crate::packstream::struct_msg(0x02, vec![]);
    let mut combined = hello.clone();
    combined.extend_from_slice(&goodbye);
    ws.send(Message::Binary(combined.into())).unwrap();

    let first = read_message(&mut adapter).unwrap();
    assert_eq!(
        crate::packstream::decode(&first).unwrap(),
        Pack::Struct {
            tag: 0x01,
            fields: vec![Pack::Map(vec![])],
        }
    );
    let second = read_message(&mut adapter).unwrap();
    assert_eq!(
        crate::packstream::decode(&second).unwrap(),
        Pack::Struct {
            tag: 0x02,
            fields: vec![],
        }
    );
}

#[test]
fn ping_gets_pong_with_same_payload() {
    let (mut ws, mut adapter) = ws_pair(WsConfig::default());
    // Drive the adapter's read side so it observes the ping and flushes the
    // queued pong before reading further input.
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 16];
        let _ = adapter.read(&mut buf);
        adapter
    });
    ws.send(Message::Ping(b"bolt-ping".to_vec().into()))
        .unwrap();
    let reply = ws.read().expect("ws error");
    assert_eq!(reply, Message::Pong(b"bolt-ping".to_vec().into()));
    drop(ws);
    reader.join().unwrap();
}

#[test]
fn ping_flood_from_a_client_that_never_reads_stalls_on_backpressure() {
    let (server, mut client) = socket_pair();
    let task = std::thread::spawn(move || accept_websocket(server, &WsConfig::default(), None));
    client
        .write_all(upgrade_request(RFC6455_KEY, "").as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(head.starts_with("HTTP/1.1 101"), "got: {head:?}");
    let mut adapter = task.join().unwrap().expect("accept must succeed");

    // The server sits in read; pings never yield Bolt bytes.
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match adapter.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });

    // The client never reads, so the moment one pong cannot be flushed the
    // server must stop reading and the client's write must stall instead of
    // growing an unbounded buffer: a flood far past every kernel buffer,
    // written in 1 MiB bursts under a 2 s write timeout, must fail.
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let ping = masked_frame(true, 0x9, &[0xAA; 125]);
    let mut burst = Vec::with_capacity(1024 * 1024 + ping.len());
    while burst.len() < 1024 * 1024 {
        burst.extend_from_slice(&ping);
    }
    let mut written = 0usize;
    let mut stalled = false;
    while written < 64 * 1024 * 1024 {
        match client.write(&burst) {
            Ok(0) | Err(_) => {
                stalled = true;
                break;
            }
            Ok(n) => written += n,
        }
    }
    assert!(
        stalled,
        "a client that never reads must stall: the server stops reading while a pong is unflushed"
    );
    drop(client);
    let _ = reader.join();
}

#[test]
fn empty_write_is_a_noop() {
    let (_ws, mut adapter) = ws_pair(WsConfig::default());
    let n = adapter.write(&[]).unwrap();
    assert_eq!(n, 0, "an empty write must report Ok(0)");
}

#[test]
fn text_frame_closes_with_1003() {
    let (mut ws, adapter) = ws_pair(WsConfig::default());
    // Reads must reach EOF once the close handshake completes.
    let reader = spawn_eof_reader(adapter);
    ws.send(Message::Text("hello".into())).unwrap();
    let close = read_until_close(&mut ws, "stream ended without close frame");
    assert_eq!(
        close.map(|f| f.code),
        Some(CloseCode::Unsupported),
        "text frame must close with 1003"
    );
    // Answer the close so the server's read side reaches EOF.
    let _ = ws.close(None);
    reader.join().unwrap();
}

#[test]
fn close_frame_is_answered_with_a_close_before_eof() {
    let (mut ws, adapter) = ws_pair(WsConfig::default());
    let reader = spawn_eof_reader(adapter);
    ws.send(Message::Close(Some(CloseFrame {
        code: CloseCode::Normal,
        reason: "done".into(),
    })))
    .unwrap();
    // The server must answer with its own Close frame before the stream
    // ends.
    let close = read_until_close(&mut ws, "stream ended without a close reply");
    assert_eq!(close.map(|f| f.code), Some(CloseCode::Normal));
    // And then the stream ends (the read guard bounds this).
    loop {
        match ws.read() {
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    reader.join().unwrap();
}

#[test]
fn goodbye_gets_a_server_close_frame_before_eof() {
    let db = Arc::new(Database::open(":memory:").unwrap());
    let (server, client_raw) = socket_pair();
    let server_task = std::thread::spawn(move || {
        serve_websocket(
            server,
            &WsConfig::default(),
            db,
            ConnConfig::default(),
            None,
        )
        .expect("serve_websocket failed");
    });
    let request = "ws://localhost/".into_client_request().unwrap();
    let (mut ws, _) = tungstenite::client(request, client_raw).expect("client handshake failed");

    // Bolt magic + version offer as one binary message.
    let mut offer = vec![0x60, 0x60, 0xB0, 0x17, 0, 0, 4, 4];
    offer.extend_from_slice(&[0; 12]);
    ws.send(Message::Binary(offer.into())).unwrap();
    match ws.read() {
        Ok(Message::Binary(v)) => assert_eq!(&v[..], &[0, 0, 4, 4]),
        other => panic!("expected the version pick, got {other:?}"),
    }
    // HELLO
    ws.send(Message::Binary(
        crate::packstream::struct_msg(0x01, crate::testutil::hello_fields()).into(),
    ))
    .unwrap();
    match ws.read() {
        Ok(Message::Binary(_)) => {}
        other => panic!("expected HELLO SUCCESS, got {other:?}"),
    }
    // GOODBYE ends the session; serve_websocket then drives the WS close.
    ws.send(Message::Binary(
        crate::packstream::struct_msg(0x02, vec![]).into(),
    ))
    .unwrap();
    let close = read_until_close(&mut ws, "stream ended without a server close frame");
    assert_eq!(close.map(|f| f.code), Some(CloseCode::Normal));
    server_task.join().expect("server task did not finish");
}

#[test]
fn get_without_valid_upgrade_gets_400() {
    let (mut client, task) = accept_thread(WsConfig::default());
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(
        head.starts_with("HTTP/1.1 400 Bad Request"),
        "expected 400, got: {head:?}"
    );
    assert!(task.join().unwrap().is_err());
}

#[test]
fn over_cap_incomplete_upgrade_request_gets_exactly_400() {
    let (mut client, task) = accept_thread(WsConfig::default());
    // 8193 bytes, one past MAX_UPGRADE_REQUEST_BYTES, with no terminating
    // blank line.
    let mut request = b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Fill: ".to_vec();
    request.resize(MAX_UPGRADE_REQUEST_BYTES + 1, b'a');
    client.write_all(&request).unwrap();
    let head = read_response_head(&mut client);
    assert_eq!(
        head, "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n",
        "an over-cap request gets exactly the 400 status response"
    );
    assert!(task.join().unwrap().is_err());
}

#[test]
fn frame_pipelined_behind_a_near_cap_request_arrives_intact() {
    let (mut client, task) = accept_thread(WsConfig::default());
    // ~8000-byte valid request (just under the 8192 cap) with a masked
    // binary frame in the SAME write: the upgrade read must not consume or
    // drop any frame byte.
    let pad = "a".repeat(7800);
    let request = upgrade_request(RFC6455_KEY, &format!("X-Pad: {pad}\r\n"));
    assert!(request.len() < MAX_UPGRADE_REQUEST_BYTES);
    let frame = masked_frame(true, 0x2, b"hello");
    let mut wire = request.into_bytes();
    wire.extend_from_slice(&frame);
    client.write_all(&wire).unwrap();

    let head = read_response_head(&mut client);
    assert!(head.starts_with("HTTP/1.1 101"), "got: {head:?}");
    let mut adapter = task.join().unwrap().expect("accept must succeed");
    let mut payload = [0u8; 5];
    adapter.read_exact(&mut payload).unwrap();
    assert_eq!(&payload, b"hello", "pipelined frame bytes must be intact");
}

#[test]
fn message_over_the_16mib_cap_ends_the_stream() {
    let (server, mut client) = socket_pair();
    let task = std::thread::spawn(move || accept_websocket(server, &WsConfig::default(), None));
    client
        .write_all(upgrade_request(RFC6455_KEY, "").as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(head.starts_with("HTTP/1.1 101"), "got: {head:?}");
    let mut adapter = task.join().unwrap().expect("accept must succeed");

    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match adapter.read(&mut buf) {
                Ok(0) => break None,
                Ok(_) => continue,
                Err(e) => break Some(e),
            }
        }
    });

    // 16 frames of exactly MAX_WS_FRAME_BYTES, then a final 1-byte frame:
    // 16 MiB + 1 byte total, one past the message cap. Every frame is
    // individually legal, so only the message cap can end the stream.
    let payload = vec![0xAA; MAX_WS_FRAME_BYTES];
    for i in 0..16 {
        if client
            .write_all(&masked_frame(
                false,
                if i == 0 { 0x2 } else { 0x0 },
                &payload,
            ))
            .is_err()
        {
            break;
        }
    }
    let _ = client.write_all(&masked_frame(true, 0x0, &[0xBB]));
    let outcome = reader.join().expect("over-cap message must end the stream");
    assert!(
        outcome.is_some(),
        "a 16 MiB + 1 byte message must fail the read side"
    );
}

#[test]
fn disallowed_origin_gets_403_and_no_101() {
    let (mut client, task) = accept_thread(WsConfig::default());
    client
        .write_all(upgrade_request(RFC6455_KEY, "Origin: https://evil.example.com\r\n").as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(
        head.starts_with("HTTP/1.1 403 Forbidden"),
        "expected 403, got: {head:?}"
    );
    assert!(task.join().unwrap().is_err());
}

#[test]
fn non_ascii_origin_fails_closed_with_403() {
    let (mut client, task) = accept_thread(WsConfig {
        origin_policy: OriginPolicy::Matched(OriginMatch::Any),
        ..WsConfig::default()
    });
    // Origin with obs-text bytes: a valid header value, but `to_str` fails.
    let mut request = format!(
        "GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {RFC6455_KEY}\r\nSec-WebSocket-Version: 13\r\nOrigin: http://"
    )
    .into_bytes();
    request.extend_from_slice(&[0xE2, 0x98, 0x83]);
    request.extend_from_slice(b"\r\n\r\n");
    client.write_all(&request).unwrap();
    let head = read_response_head(&mut client);
    assert!(
        head.starts_with("HTTP/1.1 403 Forbidden"),
        "an unparseable Origin must fail closed with 403, got: {head:?}"
    );
    assert!(task.join().unwrap().is_err());
}

#[test]
fn origin_rejection_error_is_scrubbed_and_capped() {
    let config = WsConfig {
        origin_policy: OriginPolicy::Matched(OriginMatch::Exact(vec![
            "https://app.example.com".into()
        ])),
        ..WsConfig::default()
    };
    let (mut client, task) = accept_thread(config);
    let origin = format!("https://x.example.com/<script>{}", "a".repeat(4000));
    client
        .write_all(upgrade_request(RFC6455_KEY, &format!("Origin: {origin}\r\n")).as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(head.starts_with("HTTP/1.1 403 Forbidden"), "got: {head:?}");
    let err = match task.join().unwrap() {
        Ok(_) => panic!("origin must be rejected"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.len() <= 200,
        "rejection error must be capped, got {} chars",
        err.len()
    );
    assert!(
        !err.contains('<'),
        "rejection error must not carry raw markup: {err:?}"
    );
}

#[test]
fn origin_policy_matrix() {
    // Empty list, auth off: the loopback-only default.
    let loopback = OriginPolicy::resolve(&[], false);
    assert_eq!(loopback, OriginPolicy::LoopbackOnly);
    // Loopback Origins allowed.
    for origin in [
        "http://localhost:3000",
        "http://localhost",
        "https://localhost:443",
        "http://127.0.0.1:8080",
        "https://127.0.0.1",
        "http://[::1]:9000",
        "https://[::1]",
        // The scheme is case-insensitive (RFC 3986 §3.1):
        "HTTP://localhost:3000",
    ] {
        assert!(loopback.allows(origin), "{origin} must be allowed");
    }
    // Cross-site Origins rejected — including bracketed non-IPv6 hosts,
    // userinfo, paths, and non-numeric ports.
    for origin in [
        "https://evil.example.com",
        "http://localhost.evil.com",
        "http://127.0.0.1.evil.com",
        "http://localhost:abc",
        "ftp://localhost:1",
        "http://[localhost]",
        "http://[127.0.0.1]",
        "http://user@localhost",
        "http://localhost/admin",
        // A fragment delimiter in a slash-less authority is outside the
        // shape, never a host character (round 11, F-11-3):
        "http://localhost#evil",
        "localhost",
    ] {
        assert!(!loopback.allows(origin), "{origin} must be rejected");
    }

    // Empty list, auth on: loopback plus the hosted Neo4j Browser and
    // Console — not any Origin (ADR-0459 amends ADR-0266).
    let auth_on = OriginPolicy::resolve(&[], true);
    assert_eq!(auth_on, OriginPolicy::LoopbackAndHosted);
    for origin in [
        "http://localhost:3000",
        "https://127.0.0.1:7474",
        "http://[::1]:8080",
        "https://browser.neo4j.io",
        "https://console.neo4j.io",
    ] {
        assert!(auth_on.allows(origin), "{origin} must be allowed");
    }
    for origin in [
        "https://evil.example.com",
        "https://browser.graphapp.io",
        "https://browser.neo4j.io.evil.example",
    ] {
        assert!(!auth_on.allows(origin), "{origin} must be rejected");
    }
    // An explicit wider Origin still opts in with auth on.
    let wider = OriginPolicy::resolve(
        &[OriginFlagValue::Url("https://app.example.com".to_string())],
        true,
    );
    assert!(wider.allows("https://app.example.com"));

    // '*' allows any Origin even with auth off.
    let star = OriginPolicy::resolve(&[OriginFlagValue::Wildcard], false);
    assert_eq!(star, OriginPolicy::Matched(OriginMatch::Any));
    assert!(star.allows("https://evil.example.com"));

    // An explicit list requires an exact match.
    let list = OriginPolicy::resolve(
        &[OriginFlagValue::Url("https://app.example.com".to_string())],
        false,
    );
    assert_eq!(
        list,
        OriginPolicy::Matched(OriginMatch::Exact(vec!["https://app.example.com".into()]))
    );
    assert!(list.allows("https://app.example.com"));
    // The stored entries are canonical, so a non-canonical spelling of
    // the same origin matches (RFC 3986 §3.1, §3.2.2):
    assert!(list.allows("HTTPS://APP.example.com"));
    assert!(!list.allows("https://other.example.com"));
}

#[test]
fn loopback_origin_and_missing_origin_are_accepted_with_auth_off() {
    for extra in ["Origin: http://localhost:3000\r\n", ""] {
        let (mut client, task) = accept_thread(WsConfig::default());
        client
            .write_all(upgrade_request(RFC6455_KEY, extra).as_bytes())
            .unwrap();
        let head = read_response_head(&mut client);
        assert!(
            head.starts_with("HTTP/1.1 101"),
            "expected 101 for {extra:?}, got: {head:?}"
        );
        task.join().unwrap().expect("accept must succeed");
    }
}

#[test]
fn star_allows_any_origin_over_the_wire() {
    let config = WsConfig {
        origin_policy: OriginPolicy::Matched(OriginMatch::Any),
        ..WsConfig::default()
    };
    let (mut client, task) = accept_thread(config);
    client
        .write_all(upgrade_request(RFC6455_KEY, "Origin: https://evil.example.com\r\n").as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(head.starts_with("HTTP/1.1 101"), "got: {head:?}");
    task.join().unwrap().expect("accept must succeed");
}

#[test]
fn authenticated_default_allows_hosted_and_loopback_origins_over_the_wire() {
    // ADR-0459: with auth on, an empty --ws-allow-origin list allows the
    // loopback set (any port) plus the hosted Neo4j Browser and Console —
    // and nothing else.
    let config = || WsConfig {
        origin_policy: OriginPolicy::resolve(&[], true),
        ..WsConfig::default()
    };
    for allowed in [
        "https://browser.neo4j.io",
        "https://console.neo4j.io",
        "http://localhost:3000",
        "https://127.0.0.1:7474",
        "http://[::1]:8080",
    ] {
        let (mut client, task) = accept_thread(config());
        let extra = format!("Origin: {allowed}\r\n");
        client
            .write_all(upgrade_request(RFC6455_KEY, &extra).as_bytes())
            .unwrap();
        let head = read_response_head(&mut client);
        assert!(head.starts_with("HTTP/1.1 101"), "{allowed:?}: {head:?}");
        task.join().unwrap().expect("accept must succeed");
    }
    // A missing Origin is still a non-browser client and is allowed.
    let (mut client, task) = accept_thread(config());
    client
        .write_all(upgrade_request(RFC6455_KEY, "").as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(head.starts_with("HTTP/1.1 101"), "got: {head:?}");
    task.join().unwrap().expect("accept must succeed");

    // An arbitrary Origin needs the explicit opt-in.
    let (mut client, task) = accept_thread(config());
    client
        .write_all(upgrade_request(RFC6455_KEY, "Origin: https://evil.example.com\r\n").as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(
        head.starts_with("HTTP/1.1 403 Forbidden"),
        "expected 403 for a foreign Origin, got: {head:?}"
    );
    assert!(task.join().unwrap().is_err());
}

#[test]
fn ws_connection_cap_is_off_by_default() {
    assert_eq!(WsConfig::default().connection_cap.max(), 0);
    // Two concurrent sessions are admitted with no cap configured.
    let (first, first_task) = ws_session(WsConfig::default());
    let (second, second_task) = ws_session(WsConfig::default());
    drop(first);
    drop(second);
    first_task.join().unwrap();
    second_task.join().unwrap();
}

#[test]
fn ws_connection_cap_refuses_over_cap_and_releases_slots() {
    let cap = Arc::new(ConnectionCap::new(1));
    let config = WsConfig {
        connection_cap: Arc::clone(&cap),
        ..WsConfig::default()
    };

    // The first session holds the only slot for its lifetime.
    let (first, first_task) = ws_session(config.clone());

    // A second session while the slot is held is refused before the upgrade
    // read: closed with no 101 and no HTTP response at all.
    let (server, mut client) = socket_pair();
    let second_config = config.clone();
    let second_task = std::thread::spawn(move || {
        let db = Arc::new(Database::open(":memory:").unwrap());
        let _ = serve_websocket(server, &second_config, db, ConnConfig::default(), None);
    });
    client
        .write_all(upgrade_request(RFC6455_KEY, "").as_bytes())
        .unwrap();
    let mut buf = [0u8; 64];
    match client.read(&mut buf) {
        Ok(0) | Err(_) => {}
        Ok(n) => panic!("over-cap WebSocket must be closed with no response, got {n} bytes"),
    }
    second_task.join().unwrap();
    // The slot is still the first session's.
    assert!(cap.try_acquire().is_none());

    // Ending the first session releases its slot for the next one.
    drop(first);
    first_task.join().unwrap();
    let (third, third_task) = ws_session(config);
    drop(third);
    third_task.join().unwrap();
}

#[test]
fn oversize_declared_frame_ends_stream_without_buffering_payload() {
    let (server, mut client) = socket_pair();
    let task = std::thread::spawn(move || accept_websocket(server, &WsConfig::default(), None));
    client
        .write_all(upgrade_request(RFC6455_KEY, "").as_bytes())
        .unwrap();
    let head = read_response_head(&mut client);
    assert!(head.starts_with("HTTP/1.1 101"), "got: {head:?}");
    let mut adapter = task.join().unwrap().expect("accept must succeed");

    // Masked binary frame header declaring MAX_WS_FRAME_BYTES + 1 payload
    // bytes (64-bit length form), followed by only the mask key — the
    // declared payload is never sent.
    let declared = (MAX_WS_FRAME_BYTES + 1) as u64;
    let mut header = vec![0x82u8, 0xFF];
    header.extend_from_slice(&declared.to_be_bytes());
    header.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    client.write_all(&header).unwrap();

    let mut buf = [0u8; 16];
    // The read guard on the server socket bounds this; tungstenite rejects
    // the oversize declared length at header parse, so the read fails fast.
    match adapter.read(&mut buf) {
        Ok(0) | Err(_) => {}
        Ok(n) => panic!("oversize frame must not yield {n} bytes"),
    }
}

#[test]
fn bolt_over_websocket_end_to_end() {
    let db = Arc::new(Database::open(":memory:").unwrap());
    let (client_raw, task) = accept_thread(WsConfig::default());
    std::thread::spawn(move || {
        let adapter = task.join().unwrap().expect("accept must succeed");
        handle_conn(adapter, db, ConnConfig::default(), None).expect("handle_conn failed");
    });

    let request = "ws://localhost/".into_client_request().unwrap();
    let (ws, _) = tungstenite::client(request, client_raw).expect("client handshake failed");
    let mut client = BoltWsStream::new(ws);

    crate::testutil::return_one_exchange(&mut client);
}
