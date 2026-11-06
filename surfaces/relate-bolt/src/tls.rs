//! TLS server-config construction, bind classification, and the startup
//! plaintext policy (ADR-0109). Per-connection TLS/plaintext/WebSocket
//! dispatch lives in [`crate::transport`].
//!
//! ADR-0233: sync rustls — the acceptor is just the shared
//! `Arc<ServerConfig>`; each connection builds its own `ServerConnection` +
//! `StreamOwned` over the peeked `TcpStream`.

use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::Arc;

use rustls::ServerConfig;

/// Load the TLS server config described by the three TLS flags (ADR-0109).
///
/// Returns `Ok(None)` when neither `--tls-cert` nor `--tls-key` is set. The
/// flags must be paired; `--tls-client-ca` additionally requires both.
pub(crate) fn load_tls_config(
    cert_path: Option<&Path>,
    key_path: Option<&Path>,
    client_ca_path: Option<&Path>,
) -> Result<Option<Arc<ServerConfig>>, Box<dyn Error + Send + Sync>> {
    let (cert_path, key_path) = match (cert_path, key_path) {
        (None, None) => {
            if client_ca_path.is_some() {
                return Err("--tls-client-ca requires --tls-cert and --tls-key".into());
            }
            return Ok(None);
        }
        (Some(_), None) => return Err("--tls-cert requires --tls-key".into()),
        (None, Some(_)) => return Err("--tls-key requires --tls-cert".into()),
        (Some(cert), Some(key)) => (cert, key),
    };

    let cert_file = File::open(cert_path).map_err(|e| {
        format!(
            "failed to open TLS certificate {}: {e}",
            cert_path.display()
        )
    })?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            format!(
                "failed to parse TLS certificate {}: {e}",
                cert_path.display()
            )
        })?;
    if certs.is_empty() {
        return Err(format!("no certificates found in {}", cert_path.display()).into());
    }

    let key_file = File::open(key_path)
        .map_err(|e| format!("failed to open TLS private key {}: {e}", key_path.display()))?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
        .map_err(|e| {
            format!(
                "failed to parse TLS private key {}: {e}",
                key_path.display()
            )
        })?
        .ok_or_else(|| format!("no private key found in {}", key_path.display()))?;

    let config = match client_ca_path {
        Some(ca_path) => {
            let ca_file = File::open(ca_path)
                .map_err(|e| format!("failed to open TLS client CA {}: {e}", ca_path.display()))?;
            let mut roots = rustls::RootCertStore::empty();
            for cert in rustls_pemfile::certs(&mut BufReader::new(ca_file)) {
                let cert = cert.map_err(|e| {
                    format!("failed to parse TLS client CA {}: {e}", ca_path.display())
                })?;
                roots.add(cert).map_err(|e| {
                    format!(
                        "invalid TLS client CA certificate in {}: {e}",
                        ca_path.display()
                    )
                })?;
            }
            let verifier =
                rustls::server::WebPkiClientVerifier::builder(Arc::new(roots)).build()?;
            ServerConfig::builder()
                .with_client_cert_verifier(verifier)
                .with_single_cert(certs, key)?
        }
        None => ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?,
    };

    Ok(Some(Arc::new(config)))
}

/// Resolve `--bind` and classify it as loopback only when every resolved IP is
/// a loopback address. Empty resolution is treated as non-loopback.
pub(crate) fn bind_is_loopback(bind: &str) -> std::io::Result<bool> {
    let addrs: Vec<_> = bind.to_socket_addrs()?.collect();
    Ok(!addrs.is_empty() && addrs.iter().all(|addr| addr.ip().is_loopback()))
}

/// Startup behavior for one `--bind` (ADR-0109).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupAction {
    /// Serve without any startup noise (loopback, or TLS configured without
    /// the plaintext opt-in — plaintext is then refused per connection).
    Serve,
    /// Serve, but print the plaintext warning at startup.
    WarnAndServe,
    /// Refuse to start.
    Refuse,
}

/// The single source of the plaintext matrix (ADR-0109): the startup action
/// and whether plaintext Bolt is allowed per connection, derived together so
/// the two can never drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlaintextPolicy {
    /// What `serve()` does at startup.
    pub(crate) action: StartupAction,
    /// Whether `serve_conn` serves a plaintext Bolt connection.
    pub(crate) plaintext_allowed: bool,
}

/// Decide plaintext handling (ADR-0109). Pure so the full matrix is
/// unit-testable: loopback always serves silently; non-loopback without TLS
/// and without `--allow-plaintext` refuses; `--allow-plaintext` on a
/// non-loopback bind warns whether TLS is configured or not.
pub(crate) fn startup_plaintext_policy(
    bind_loopback: bool,
    tls_configured: bool,
    allow_plaintext: bool,
) -> PlaintextPolicy {
    let plaintext_allowed = bind_loopback || allow_plaintext;
    let action = if bind_loopback {
        StartupAction::Serve
    } else if allow_plaintext {
        StartupAction::WarnAndServe
    } else if !tls_configured {
        StartupAction::Refuse
    } else {
        StartupAction::Serve
    };
    PlaintextPolicy {
        action,
        plaintext_allowed,
    }
}

#[cfg(test)]
#[path = "tls_tests.rs"]
mod tests;
