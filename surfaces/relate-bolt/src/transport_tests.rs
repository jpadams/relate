use super::*;
use crate::protocol::ConnLimits;
use crate::testutil::upgrade_request;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn classify_routes_bolt_and_websocket() {
    let t = Transport {
        tls: None,
        plaintext_allowed: true,
        websocket: Some(Arc::new(WsConfig::default())),
    };
    for plaintext in [true, false] {
        assert!(matches!(classify(0x60, &t, plaintext), Dispatch::Bolt));
        assert!(matches!(
            classify(0x47, &t, plaintext),
            Dispatch::WebSocket(_)
        ));
        assert!(matches!(
            classify(0x00, &t, plaintext),
            Dispatch::Close(Close::Unrecognized(0x00))
        ));
    }
}

#[test]
fn no_websocket_closes_0x47_identically_before_and_after_tls() {
    let t = Transport {
        tls: None,
        plaintext_allowed: true,
        websocket: None,
    };
    for plaintext in [true, false] {
        assert!(
            matches!(
                classify(0x47, &t, plaintext),
                Dispatch::Close(Close::WebSocketDisabled)
            ),
            "0x47 with --no-websocket must close (plaintext={plaintext})"
        );
        assert!(matches!(classify(0x60, &t, plaintext), Dispatch::Bolt));
    }
}

#[test]
fn plaintext_policy_gates_bolt_and_websocket_but_not_tls() {
    let t = Transport {
        tls: None,
        plaintext_allowed: false,
        websocket: Some(Arc::new(WsConfig::default())),
    };
    // Plaintext stream: refused by the ADR-0109 policy.
    assert!(matches!(
        classify(0x60, &t, true),
        Dispatch::Close(Close::RefusedPlaintextBolt)
    ));
    assert!(matches!(
        classify(0x47, &t, true),
        Dispatch::Close(Close::RefusedPlaintextWebSocket)
    ));
    // Decrypted TLS stream: served.
    assert!(matches!(classify(0x60, &t, false), Dispatch::Bolt));
    assert!(matches!(classify(0x47, &t, false), Dispatch::WebSocket(_)));
}

/// Each close cause logs its own line; the refusal diagnostics name the
/// flag that would have served the connection (byte-identical to the
/// pre-transport-extraction messages).
#[test]
fn close_causes_log_their_own_diagnostics() {
    assert_eq!(
        Close::RefusedPlaintextWebSocket.log(),
        "bolt: refusing plaintext WebSocket on a non-loopback bind without --allow-plaintext; closing"
    );
    assert_eq!(
        Close::RefusedPlaintextBolt.log(),
        "bolt: refusing plaintext Bolt on a non-loopback bind without --allow-plaintext; closing"
    );
    assert_eq!(
        Close::WebSocketDisabled.log(),
        "bolt: websocket upgrade received but WebSocket is disabled (--no-websocket); closing"
    );
    assert_eq!(
        Close::Unrecognized(0x00).log(),
        "bolt: unrecognized first byte 0x00; closing connection"
    );
}

/// Spawn a one-connection server thread running `serve_conn` with
/// `transport`; return the address to connect to.
fn spawn_transport(transport: Transport, config: ConnConfig) -> std::net::SocketAddr {
    let db = Arc::new(Database::open(":memory:").unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        serve_conn(stream, &transport, db, config).unwrap();
    });
    addr
}

/// A refusal assertion: the server must close without sending a byte. The
/// read guard makes a misbehaving server a test failure, not a hang.
fn assert_closed_without_reply(client: &mut TcpStream, what: &str) {
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut buf = [0u8; 64];
    match client.read(&mut buf) {
        Ok(0) => {}
        Ok(n) => panic!("{what} must receive no reply, got {n} bytes"),
        Err(e) if is_timeout(&e) => panic!("{what}: the server must close the connection"),
        Err(_) => {}
    }
}

#[test]
fn websocket_plaintext_is_refused_when_not_allowed() {
    let addr = spawn_transport(
        Transport {
            tls: None,
            plaintext_allowed: false,
            websocket: Some(Arc::new(WsConfig::default())),
        },
        ConnConfig::default(),
    );

    let mut client = TcpStream::connect(addr).unwrap();
    client
        .write_all(upgrade_request("dGhlIHNhbXBsZSBub25jZQ==", "").as_bytes())
        .unwrap();
    assert_closed_without_reply(&mut client, "plaintext ws://");
}

#[test]
fn plaintext_bolt_is_refused_when_not_allowed() {
    let addr = spawn_transport(
        Transport {
            tls: None,
            plaintext_allowed: false,
            websocket: Some(Arc::new(WsConfig::default())),
        },
        ConnConfig::default(),
    );

    let mut client = TcpStream::connect(addr).unwrap();
    let mut offer = [0u8; 20];
    offer[..4].copy_from_slice(&[0x60, 0x60, 0xB0, 0x17]);
    offer[4..8].copy_from_slice(&[0, 0, 4, 4]);
    client.write_all(&offer).unwrap();
    assert_closed_without_reply(&mut client, "plaintext Bolt");
}

/// A self-signed `localhost` server identity: the TLS config plus the DER
/// the client trusts.
fn self_signed_server(dir: &Path) -> (Arc<ServerConfig>, CertificateDer<'static>) {
    let key = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let cert = params.self_signed(&key).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, key.serialize_pem()).unwrap();
    let config = crate::tls::load_tls_config(Some(&cert_path), Some(&key_path), None)
        .unwrap()
        .unwrap();
    (config, cert.der().clone())
}

/// A sync TLS client over `tcp` trusting `der`; the handshake is driven to
/// completion before the stream is returned.
fn tls_client(
    tcp: TcpStream,
    der: CertificateDer<'static>,
) -> StreamOwned<ClientConnection, TcpStream> {
    let mut roots = RootCertStore::empty();
    roots.add(der).unwrap();
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from("localhost").unwrap();
    let mut conn = ClientConnection::new(Arc::new(config), name).unwrap();
    while conn.is_handshaking() {
        conn.complete_io(&mut &tcp).unwrap();
    }
    StreamOwned::new(conn, tcp)
}

/// ADR-0141: one accept-time deadline spans the TLS handshake AND the
/// WebSocket upgrade read. A client that completes TLS and then stalls
/// mid-upgrade is closed when the single 1 s budget elapses — per-phase
/// budgets would grant a fresh second after the TLS handshake and close
/// at ~2 s.
#[test]
fn single_deadline_spans_tls_handshake_and_upgrade_read() {
    let dir = tempfile::tempdir().unwrap();
    let (server_config, der) = self_signed_server(dir.path());
    let config = ConnConfig {
        limits: ConnLimits {
            handshake_timeout: Some(Duration::from_secs(1)),
            ..ConnLimits::default()
        },
        ..ConnConfig::default()
    };
    let addr = spawn_transport(
        Transport {
            tls: Some(server_config),
            plaintext_allowed: false,
            websocket: Some(Arc::new(WsConfig::default())),
        },
        config,
    );
    let accepted_at = Instant::now();

    let tcp = TcpStream::connect(addr).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut tls = tls_client(tcp, der);
    // Stall mid-upgrade: a partial request, never terminated.
    tls.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n")
        .unwrap();
    tls.flush().unwrap();

    let mut buf = [0u8; 1];
    let closed = tls.read(&mut buf);
    let elapsed = accepted_at.elapsed();
    assert!(
        matches!(closed, Ok(0) | Err(_)),
        "the stalled upgrade must be closed by the deadline: {closed:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(900),
        "closed before the 1 s deadline elapsed: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_millis(1500),
        "closed at {elapsed:?}: past one 1 s budget, so phases still get fresh budgets"
    );
}
