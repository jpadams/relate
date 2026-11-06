use super::*;
use crate::cli_args::Args;
use crate::packstream::Pack;
use crate::protocol::ConnConfig;
use crate::server::serve;
use crate::testutil::{read_msg, success_map, try_read_msg};
use crate::transport::{serve_conn, Transport};
use clap::Parser;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose,
};
use relate::Database;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// A CA generated in-test. No PEM fixtures are committed; every test builds
/// its own hierarchy with rcgen.
struct TestCa {
    params: CertificateParams,
    key: KeyPair,
    cert_pem: String,
    der: CertificateDer<'static>,
}

impl TestCa {
    fn generate(common_name: &str) -> TestCa {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, common_name);
        params.distinguished_name = dn;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let cert = params.self_signed(&key).unwrap();
        TestCa {
            params,
            key,
            cert_pem: cert.pem(),
            der: cert.der().clone(),
        }
    }

    fn issuer(&self) -> Issuer<'_, &KeyPair> {
        Issuer::from_params(&self.params, &self.key)
    }

    /// Server identity as PEM (cert chain and PKCS#8 key) for
    /// `load_tls_config`.
    fn server_pems(&self, san: &str) -> (String, String) {
        let key = KeyPair::generate().unwrap();
        let params = CertificateParams::new(vec![san.to_string()]).unwrap();
        let cert = params.signed_by(&key, &self.issuer()).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    /// Client identity as DER for a rustls `ClientConfig`.
    fn client_identity(&self) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let cert = params.signed_by(&key, &self.issuer()).unwrap();
        (
            cert.der().clone(),
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
    }
}

fn write_pem(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

/// Build a TLS server config for `ca` through the real `load_tls_config`
/// path.
fn config_for(dir: &Path, ca: &TestCa, client_ca: Option<&Path>) -> Arc<ServerConfig> {
    let (cert_pem, key_pem) = ca.server_pems("localhost");
    let cert = write_pem(dir, "server-cert.pem", &cert_pem);
    let key = write_pem(dir, "server-key.pem", &key_pem);
    load_tls_config(Some(&cert), Some(&key), client_ca)
        .unwrap()
        .unwrap()
}

fn roots_trusting(ca: &TestCa) -> RootCertStore {
    let mut roots = RootCertStore::empty();
    roots.add(ca.der.clone()).unwrap();
    roots
}

fn client_config(
    roots: RootCertStore,
    identity: Option<(CertificateDer<'static>, PrivateKeyDer<'static>)>,
) -> ClientConfig {
    let builder = ClientConfig::builder().with_root_certificates(roots);
    match identity {
        Some((cert, key)) => builder.with_client_auth_cert(vec![cert], key).unwrap(),
        None => builder.with_no_client_auth(),
    }
}

/// Connect over TLS, driving the handshake to completion; `Err` means the
/// handshake failed.
fn tls_connect(
    addr: std::net::SocketAddr,
    config: ClientConfig,
) -> std::io::Result<StreamOwned<ClientConnection, TcpStream>> {
    let tcp = TcpStream::connect(addr)?;
    tcp.set_read_timeout(Some(Duration::from_secs(5)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(5)))?;
    let name = ServerName::try_from("localhost").unwrap();
    let mut conn = ClientConnection::new(Arc::new(config), name).map_err(std::io::Error::other)?;
    while conn.is_handshaking() {
        conn.complete_io(&mut &tcp)?;
    }
    Ok(StreamOwned::new(conn, tcp))
}

/// Connect over TLS, send HELLO, and return the first decoded server
/// message. `Err` means the handshake (or the first exchange) failed.
fn tls_hello(addr: std::net::SocketAddr, config: ClientConfig) -> std::io::Result<Pack> {
    let mut client = tls_connect(addr, config)?;

    // Bolt 4.4 version negotiation via the shared fallible helper so a
    // rejected client fails gracefully instead of panicking.
    crate::testutil::try_handshake(&mut client)?;

    let hello = crate::packstream::struct_msg(0x01, crate::testutil::hello_fields());
    client.write_all(&hello)?;
    try_read_msg(&mut client)
}

/// Run one `serve_conn` session with `server_config` and probe it with
/// `config`.
fn mtls_probe(server_config: Arc<ServerConfig>, config: ClientConfig) -> std::io::Result<Pack> {
    let db = Arc::new(Database::open(":memory:").unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let t = Transport {
            tls: Some(server_config),
            plaintext_allowed: false,
            websocket: Some(Arc::new(crate::websocket::WsConfig::default())),
        };
        let _ = serve_conn(stream, &t, db, ConnConfig::default());
    });
    tls_hello(addr, config)
}

#[test]
fn tls_flags_must_be_paired() {
    assert!(load_tls_config(None, None, None).unwrap().is_none());

    let e = load_tls_config(Some(Path::new("cert.pem")), None, None)
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("--tls-key"), "{e}");

    let e = load_tls_config(None, Some(Path::new("key.pem")), None)
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("--tls-cert"), "{e}");

    let e = load_tls_config(None, None, Some(Path::new("ca.pem")))
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("--tls-client-ca"), "{e}");
}

#[test]
fn unloadable_certificate_names_the_path() {
    let e = load_tls_config(
        Some(Path::new("does-not-exist-cert.pem")),
        Some(Path::new("does-not-exist-key.pem")),
        None,
    )
    .err()
    .unwrap()
    .to_string();
    assert!(e.contains("does-not-exist-cert.pem"), "{e}");
}

#[test]
fn bind_classification_covers_loopback_and_wildcard() {
    assert!(bind_is_loopback("127.0.0.1:7687").unwrap());
    assert!(bind_is_loopback("[::1]:7687").unwrap());
    assert!(bind_is_loopback("localhost:7687").unwrap());
    assert!(!bind_is_loopback("0.0.0.0:7687").unwrap());
    assert!(!bind_is_loopback("192.168.1.10:7687").unwrap());
}

#[test]
fn tls_end_to_end_serves_bolt_over_tls() {
    let dir = tempfile::tempdir().unwrap();
    let ca = TestCa::generate("test server ca");
    let server_config = config_for(dir.path(), &ca, None);
    let db = Arc::new(Database::open(":memory:").unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let t = Transport {
            tls: Some(server_config),
            plaintext_allowed: false,
            websocket: Some(Arc::new(crate::websocket::WsConfig::default())),
        };
        serve_conn(stream, &t, db, ConnConfig::default()).unwrap();
    });

    let mut client =
        tls_connect(addr, client_config(roots_trusting(&ca), None)).expect("TLS handshake failed");

    crate::testutil::handshake(&mut client);

    // HELLO
    client
        .write_all(&crate::packstream::struct_msg(
            0x01,
            crate::testutil::hello_fields(),
        ))
        .unwrap();
    success_map(read_msg(&mut client));

    // RUN RETURN 1 AS n
    client
        .write_all(&crate::packstream::struct_msg(
            0x10,
            vec![Pack::String("RETURN 1 AS n".into()), Pack::Map(vec![])],
        ))
        .unwrap();
    success_map(read_msg(&mut client));

    // PULL {n: -1}
    client
        .write_all(&crate::packstream::struct_msg(
            0x3F,
            vec![Pack::Map(vec![(Pack::String("n".into()), Pack::Int(-1))])],
        ))
        .unwrap();
    assert_eq!(
        read_msg(&mut client),
        Pack::Struct {
            tag: 0x71,
            fields: vec![Pack::List(vec![Pack::Int(1)])],
        }
    );
    let m = success_map(read_msg(&mut client));
    assert!(m.iter().any(|(k, _)| k.as_str() == Some("t_last")));

    drop(client);
    server.join().expect("server thread did not finish");
}

#[test]
fn mtls_requires_a_client_cert_signed_by_the_configured_ca() {
    let dir = tempfile::tempdir().unwrap();
    let server_ca = TestCa::generate("mtls server ca");
    let client_ca = TestCa::generate("mtls client ca");
    let other_ca = TestCa::generate("mtls other ca");
    let client_ca_path = write_pem(dir.path(), "client-ca.pem", &client_ca.cert_pem);
    let server_config = config_for(dir.path(), &server_ca, Some(&client_ca_path));

    // (a) A client cert signed by the configured CA completes the handshake.
    let trusted = mtls_probe(
        Arc::clone(&server_config),
        client_config(
            roots_trusting(&server_ca),
            Some(client_ca.client_identity()),
        ),
    );
    assert!(
        matches!(trusted, Ok(Pack::Struct { tag: 0x70, .. })),
        "CA-signed client cert must complete the handshake and HELLO: {trusted:?}"
    );

    // (b) No client cert fails the handshake.
    let missing = mtls_probe(
        Arc::clone(&server_config),
        client_config(roots_trusting(&server_ca), None),
    );
    assert!(
        missing.is_err(),
        "a client without a certificate must be rejected"
    );

    // (c) A cert signed by a different CA fails the handshake.
    let wrong_ca = mtls_probe(
        server_config,
        client_config(roots_trusting(&server_ca), Some(other_ca.client_identity())),
    );
    assert!(
        wrong_ca.is_err(),
        "a client cert signed by another CA must be rejected"
    );
}

#[test]
fn plaintext_is_served_through_the_peek_path_on_a_tls_listener() {
    let dir = tempfile::tempdir().unwrap();
    let ca = TestCa::generate("peek ca");
    let server_config = config_for(dir.path(), &ca, None);
    let db = Arc::new(Database::open(":memory:").unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let t = Transport {
            tls: Some(server_config),
            plaintext_allowed: true,
            websocket: Some(Arc::new(crate::websocket::WsConfig::default())),
        };
        serve_conn(stream, &t, db, ConnConfig::default()).unwrap();
    });

    // Plaintext Bolt on a TLS-enabled listener: the peek must not consume
    // the 0x60 magic byte.
    let mut client = crate::testutil::connect(addr);
    crate::testutil::handshake(&mut client);
    client
        .write_all(&crate::packstream::struct_msg(
            0x01,
            crate::testutil::hello_fields(),
        ))
        .unwrap();
    assert!(matches!(
        read_msg(&mut client),
        Pack::Struct { tag: 0x70, .. }
    ));
}

#[test]
fn startup_refuses_non_loopback_plaintext_and_names_remedies() {
    let args = Args::try_parse_from(["relate-bolt", "--bind", "0.0.0.0:7687"]).unwrap();
    let err = serve(&args).expect_err("non-loopback plaintext must be refused");
    let msg = err.to_string();
    for remedy in ["--tls-cert", "--tls-key", "--allow-plaintext"] {
        assert!(msg.contains(remedy), "{remedy} missing from: {msg}");
    }
}

#[test]
fn startup_plaintext_policy_covers_the_full_matrix() {
    // Loopback serves silently with plaintext allowed, regardless of TLS
    // / --allow-plaintext.
    for tls_configured in [false, true] {
        for allow_plaintext in [false, true] {
            let policy = startup_plaintext_policy(true, tls_configured, allow_plaintext);
            assert_eq!(
                policy.action,
                StartupAction::Serve,
                "loopback must serve silently (tls={tls_configured}, allow={allow_plaintext})"
            );
            assert!(
                policy.plaintext_allowed,
                "loopback must allow plaintext (tls={tls_configured}, allow={allow_plaintext})"
            );
        }
    }

    // Non-loopback without TLS and without the opt-in refuses to start.
    let policy = startup_plaintext_policy(false, false, false);
    assert_eq!(policy.action, StartupAction::Refuse);
    assert!(!policy.plaintext_allowed);

    // Non-loopback with the opt-in warns, TLS configured or not.
    let policy = startup_plaintext_policy(false, false, true);
    assert_eq!(policy.action, StartupAction::WarnAndServe);
    assert!(policy.plaintext_allowed);
    // Regression: previously silent when TLS was configured.
    let policy = startup_plaintext_policy(false, true, true);
    assert_eq!(
        policy.action,
        StartupAction::WarnAndServe,
        "non-loopback + TLS + --allow-plaintext must warn (previously silent)"
    );
    assert!(policy.plaintext_allowed);

    // Non-loopback with TLS and no opt-in serves; plaintext is refused
    // per connection by serve_conn.
    let policy = startup_plaintext_policy(false, true, false);
    assert_eq!(policy.action, StartupAction::Serve);
    assert!(
        !policy.plaintext_allowed,
        "non-loopback + TLS without the opt-in must refuse plaintext per connection"
    );
}

#[test]
fn tls_help_lists_tls_and_plaintext_flags() {
    let help = Args::try_parse_from(["relate-bolt", "--help"])
        .unwrap_err()
        .to_string();
    for flag in [
        "--tls-cert",
        "--tls-key",
        "--tls-client-ca",
        "--allow-plaintext",
    ] {
        assert!(help.contains(flag), "{flag} missing from help:\n{help}");
    }
}
