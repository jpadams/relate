//! Per-connection transport dispatch (ADR-0109, ADR-0266, ADR-0141).
//!
//! [`serve_conn`] owns everything between `accept` and the Bolt session: it
//! computes the single ADR-0141 handshake deadline, peeks the first byte,
//! runs the TLS handshake when the byte is a ClientHello record, and routes
//! the (decrypted) first byte through one shared [`classify`] — Bolt,
//! WebSocket, or close — so `--no-websocket` behaves identically before and
//! after TLS by construction.
//!
//! ADR-0233: everything here is blocking I/O on the connection's own thread.
//! The async timeout plumbing is replaced by socket read/write deadlines,
//! re-armed with the remaining deadline budget before each negotiation phase
//! ([`arm_phase`]).

use std::error::Error;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Instant;

use relate::Database;
use rustls::{ServerConfig, ServerConnection, StreamOwned};

use crate::protocol::{handle_conn, ConnConfig};
use crate::websocket::{serve_websocket, WsConfig};

/// Access to the socket underneath a served stream (ADR-0233). The async
/// `with_timeout`/`with_deadline` future plumbing is replaced by socket read
/// deadlines (`SO_RCVTIMEO`); every stream `handle_conn` can serve — a raw
/// `TcpStream`, the post-TLS `BufReader<StreamOwned<..>>`, and the WebSocket
/// adapter — exposes them through this.
pub trait SocketTimeouts {
    /// Set the per-read timeout (`None` disables).
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()>;
    /// Set the per-write timeout (`None` disables).
    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()>;
}

impl SocketTimeouts for TcpStream {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        TcpStream::set_read_timeout(self, dur)
    }
    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        TcpStream::set_write_timeout(self, dur)
    }
}

/// A `&mut` borrow delegates, so a caller holding a mutable borrow (e.g.
/// `serve_websocket` mid-session) still satisfies the `handle_conn` bound.
impl<T: SocketTimeouts + ?Sized> SocketTimeouts for &mut T {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        (**self).set_read_timeout(dur)
    }
    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        (**self).set_write_timeout(dur)
    }
}

/// True when `e` is a socket read/write deadline expiring (the sync form of
/// a `tokio::time::timeout` elapsing).
pub(crate) fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// The one negotiation-deadline computation (ADR-0141): the accept-time
/// deadline every negotiation phase shares — `now + limits.handshake_timeout`,
/// or `None` when the handshake timeout is disabled (ADR-0141's 0-disables).
/// `serve_conn` calls this once at accept and passes the result down to
/// `handle_conn` and `serve_websocket` as plain data.
pub(crate) fn negotiation_deadline(limits: &crate::protocol::ConnLimits) -> Option<Instant> {
    limits.handshake_timeout.map(|d| Instant::now() + d)
}

/// Arm one negotiation phase against the single ADR-0141 accept-time
/// deadline (ADR-0233): recompute the remaining budget and install it as the
/// socket's read AND write timeouts, so a phase's blocking I/O fails with a
/// timeout error the moment the shared budget is spent. Returns `false` when
/// the deadline is set and no budget remains — the phase must not start.
/// `None` disables the bound (direct-call tests).
pub(crate) fn arm_phase<S: SocketTimeouts + ?Sized>(
    stream: &mut S,
    deadline: Option<Instant>,
) -> std::io::Result<bool> {
    match deadline {
        None => {
            // Install failures propagate (the file's policy since round 3):
            // a timeout that silently never installed leaves a wait
            // unbounded.
            stream.set_read_timeout(None)?;
            stream.set_write_timeout(None)?;
            Ok(true)
        }
        Some(d) => {
            let remaining = d.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }
            stream.set_read_timeout(Some(remaining))?;
            stream.set_write_timeout(Some(remaining))?;
            Ok(true)
        }
    }
}

/// Re-arm `stream` with the budget remaining to `deadline` before one read
/// of a negotiation loop, so a client trickling a byte per read window
/// cannot outlive the ADR-0141 deadline (a per-syscall timeout alone would
/// grant it N × the budget). `None` (no shared deadline — direct-call
/// tests) leaves the installed timeouts untouched. An exhausted budget is
/// a timeout-kind error, so the caller's `is_timeout` mapping closes the
/// connection.
pub(crate) fn arm_or_timeout<S: SocketTimeouts + ?Sized>(
    stream: &mut S,
    deadline: Option<Instant>,
) -> std::io::Result<()> {
    if deadline.is_none() {
        // No shared deadline (direct-call tests): leave the installed
        // timeouts untouched.
        return Ok(());
    }
    if arm_phase(stream, deadline)? {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::WouldBlock,
        "the negotiation deadline is spent",
    ))
}

/// Run one negotiation phase under the shared accept-time deadline: arm its
/// remaining budget, then run `io`. A deadline expiry — at arm time or as
/// a mid-I/O timeout error — logs `message` once and closes the connection
/// (`Ok(None)`); other I/O errors propagate. `Ok(Some(_))` is the completed
/// phase. Every negotiation phase — the peeks, the TLS handshake, and the
/// Bolt magic/version exchange in protocol.rs — writes its budget and its
/// timeout diagnostic exactly once, here.
pub(crate) fn phase<S: SocketTimeouts + ?Sized, T>(
    stream: &mut S,
    deadline: Option<Instant>,
    message: &str,
    io: impl FnOnce(&mut S) -> std::io::Result<T>,
) -> Result<Option<T>, Box<dyn Error + Send + Sync>> {
    if !arm_phase(stream, deadline)? {
        eprintln!("{message}");
        return Ok(None);
    }
    match io(stream) {
        Ok(value) => Ok(Some(value)),
        Err(e) if is_timeout(&e) => {
            eprintln!("{message}");
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

/// The decrypted stream after the TLS accept: a `BufReader` so the post-TLS
/// first-byte re-sniff can `fill_buf` without consuming (ADR-0266), with
/// `Write` delegated to the TLS stream (std `BufReader` has no `Write`
/// impl) and the socket timeouts forwarded to the `TcpStream` underneath.
pub(crate) struct TlsStream {
    reader: BufReader<StreamOwned<ServerConnection, TcpStream>>,
}

impl TlsStream {
    fn new(stream: StreamOwned<ServerConnection, TcpStream>) -> Self {
        Self {
            reader: BufReader::new(stream),
        }
    }

    /// The first decrypted byte(s) without consuming them (the post-TLS
    /// re-sniff).
    fn peek_decrypted(&mut self) -> std::io::Result<&[u8]> {
        self.reader.fill_buf()
    }
}

impl std::io::Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}

impl std::io::Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.reader.get_mut().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.reader.get_mut().flush()
    }
}

/// The in-flight handshake stream: timeouts land on the socket underneath.
impl SocketTimeouts for StreamOwned<ServerConnection, TcpStream> {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        self.sock.set_read_timeout(dur)
    }
    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        self.sock.set_write_timeout(dur)
    }
}

impl SocketTimeouts for TlsStream {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        self.reader.get_mut().sock.set_read_timeout(dur)
    }
    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        self.reader.get_mut().sock.set_write_timeout(dur)
    }
}

/// The transport-level serving bundle: everything [`serve_conn`] needs that
/// is not per-connection protocol state (which stays in [`ConnConfig`]).
/// The `WsConfig` sits behind an `Arc` so no connection deep-clones the
/// Origin list.
#[derive(Clone)]
pub struct Transport {
    /// TLS server config; `None` when `--tls-cert`/`--tls-key` are unset.
    pub tls: Option<Arc<ServerConfig>>,
    /// Whether the plaintext path (raw Bolt or `ws://`) is served
    /// (ADR-0109).
    pub plaintext_allowed: bool,
    /// WebSocket serving (ADR-0266): `Some` when enabled (the default),
    /// `None` after `--no-websocket`.
    pub websocket: Option<Arc<WsConfig>>,
}

impl Transport {
    /// The embedded loopback default (`relate serve --bolt`, where the
    /// CLI binds the socket and shares the open store): plaintext loopback,
    /// bolt's sanctioned default (ADR-0109); no TLS; the caller's composed
    /// WebSocket policy. The caller is the composition root — it resolves
    /// the bundle with [`WsConfig::resolve`] — so this type only carries it.
    pub fn loopback_embedded(websocket: WsConfig) -> Self {
        Transport {
            tls: None,
            plaintext_allowed: true,
            websocket: Some(Arc::new(websocket)),
        }
    }
}

/// What one connection's first byte dispatches to.
enum Dispatch<'a> {
    /// Bolt magic preamble: serve the Bolt session.
    Bolt,
    /// `G` of `GET`: a WebSocket upgrade under this config.
    WebSocket(&'a WsConfig),
    /// Nothing to serve: close, logging the cause's own diagnostic.
    Close(Close),
}

/// Why a connection is closed without being served. [`classify`] decides
/// the cause; each cause logs its own line — the ADR-0109 refusal names
/// `--allow-plaintext`, the ADR-0266 disabled upgrade names `--no-websocket`
/// — so the log never states a cause that did not happen.
enum Close {
    /// A Bolt first byte (`0x60`) on a plaintext stream the ADR-0109
    /// policy refuses (non-loopback without `--allow-plaintext`).
    RefusedPlaintextBolt,
    /// A WebSocket first byte (`0x47`) on a plaintext stream the ADR-0109
    /// policy refuses.
    RefusedPlaintextWebSocket,
    /// `0x47` with `--no-websocket`.
    WebSocketDisabled,
    /// A TLS ClientHello with no `--tls-cert`/`--tls-key`.
    TlsNotConfigured,
    /// Any other first byte.
    Unrecognized(u8),
}

impl Close {
    /// The one log line for this close cause.
    fn log(&self) -> String {
        match self {
            Close::RefusedPlaintextBolt => {
                "bolt: refusing plaintext Bolt on a non-loopback bind without --allow-plaintext; closing".to_string()
            }
            Close::RefusedPlaintextWebSocket => {
                "bolt: refusing plaintext WebSocket on a non-loopback bind without --allow-plaintext; closing".to_string()
            }
            Close::WebSocketDisabled => {
                "bolt: websocket upgrade received but WebSocket is disabled (--no-websocket); closing".to_string()
            }
            Close::TlsNotConfigured => {
                "bolt: TLS ClientHello received but TLS is not configured; closing".to_string()
            }
            Close::Unrecognized(byte) => {
                format!("bolt: unrecognized first byte {byte:#04x}; closing connection")
            }
        }
    }
}

/// Classify a connection's first byte against the transport config. The one
/// classifier for BOTH the plaintext stream (`plaintext: true`, subject to
/// the ADR-0109 plaintext policy) and the decrypted TLS stream
/// (`plaintext: false` — TLS is itself the encryption, so the plaintext
/// policy no longer applies).
fn classify(first_byte: u8, transport: &Transport, plaintext: bool) -> Dispatch<'_> {
    let allowed = !plaintext || transport.plaintext_allowed;
    match first_byte {
        // Bolt magic preamble.
        0x60 if allowed => Dispatch::Bolt,
        // `G` of `GET`: a WebSocket upgrade when enabled.
        0x47 if allowed => match &transport.websocket {
            Some(ws) => Dispatch::WebSocket(ws),
            None => Dispatch::Close(Close::WebSocketDisabled),
        },
        // A Bolt or WebSocket byte refused by the plaintext policy.
        0x60 => Dispatch::Close(Close::RefusedPlaintextBolt),
        0x47 => Dispatch::Close(Close::RefusedPlaintextWebSocket),
        byte => Dispatch::Close(Close::Unrecognized(byte)),
    }
}

/// The one post-classify dispatch, shared by the plaintext and post-TLS
/// paths: route the classified decision to Bolt, WebSocket, or a close.
fn dispatch<S: std::io::Read + std::io::Write + SocketTimeouts>(
    decision: Dispatch<'_>,
    stream: S,
    db: Arc<Database>,
    config: ConnConfig,
    deadline: Option<Instant>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match decision {
        Dispatch::Bolt => handle_conn(stream, db, config, deadline),
        Dispatch::WebSocket(ws) => serve_websocket(stream, ws, db, config, deadline),
        Dispatch::Close(cause) => {
            eprintln!("{}", cause.log());
            Ok(())
        }
    }
}

/// Serve one accepted connection, choosing TLS or plaintext by peeking the
/// first byte (ADR-0109) and routing WebSocket upgrades (ADR-0266).
///
/// ADR-0141: one deadline — `now + limits.handshake_timeout` — is computed
/// here, at accept, and covers every negotiation phase: the first-byte peek,
/// the TLS handshake, the post-TLS first-byte peek, the WebSocket upgrade
/// read (inside [`serve_websocket`]), and the Bolt magic/version exchange
/// (inside [`handle_conn`]). A phase that starts with no budget left fails
/// immediately; when the version exchange completes, the deadline is
/// discharged and the idle per-read timeout governs the session.
///
/// Neither peek consumes: `TcpStream::peek` leaves the first byte for the
/// TLS handshake or the Bolt magic read, and the post-TLS sniff uses
/// `BufReader::fill_buf`, handing the `BufReader` on with the byte still
/// buffered.
pub fn serve_conn(
    mut stream: TcpStream,
    transport: &Transport,
    db: Arc<Database>,
    config: ConnConfig,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // ADR-0140: the ROUTE fallback address is this connection's own local
    // address; resolve it here, at the transport boundary, while the socket
    // is still a `TcpStream` (`handle_conn` is generic over the stream). A
    // connected socket without a local address is broken anyway: log and
    // close rather than serving a connection that cannot answer ROUTE.
    let mut config = config;
    match stream.local_addr() {
        Ok(local) => config.local_address = local.to_string(),
        Err(e) => {
            eprintln!("bolt: cannot resolve the connection's local address ({e}); closing");
            return Ok(());
        }
    }
    // ADR-0141: the single negotiation deadline, computed once at accept by
    // the one protocol.rs helper and passed down as plain data so the
    // WebSocket upgrade read (`serve_websocket`) and the magic/version
    // exchange (`handle_conn`) share it with the peeks and the TLS
    // handshake below.
    let deadline = negotiation_deadline(&config.limits);

    // Phase: first-byte peek. `peek` waits until at least one byte is
    // readable (bounded by the re-armed read deadline); 0 means EOF.
    let mut first = [0u8; 1];
    let Some(readable) = phase(
        &mut stream,
        deadline,
        "bolt: handshake timeout waiting for the first byte; closing connection",
        |s| s.peek(&mut first),
    )?
    else {
        return Ok(());
    };
    if readable == 0 {
        return Ok(());
    }
    match first[0] {
        // TLS ClientHello record.
        0x16 => match &transport.tls {
            Some(server_config) => serve_tls_conn(
                stream,
                Arc::clone(server_config),
                transport,
                db,
                config,
                deadline,
            ),
            None => dispatch(
                Dispatch::Close(Close::TlsNotConfigured),
                stream,
                db,
                config,
                deadline,
            ),
        },
        byte => dispatch(
            classify(byte, transport, true),
            stream,
            db,
            config,
            deadline,
        ),
    }
}

/// The TLS arm of [`serve_conn`] (a `0x16` ClientHello with TLS
/// configured): the handshake under the remaining deadline budget, then
/// the second first-byte sniff on the decrypted stream (ADR-0266) and the
/// shared dispatch.
fn serve_tls_conn(
    stream: TcpStream,
    server_config: Arc<ServerConfig>,
    transport: &Transport,
    db: Arc<Database>,
    config: ConnConfig,
    deadline: Option<Instant>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let conn = ServerConnection::new(server_config)?;
    let mut tls = StreamOwned::new(conn, stream);
    // Phase: the TLS handshake, under the remaining budget.
    let handshake = phase(
        &mut tls,
        deadline,
        "bolt: TLS handshake timeout; closing connection",
        |t| {
            while t.conn.is_handshaking() {
                // Re-arm per iteration: the loop is bounded by the
                // wall-clock deadline, not by syscall count (a trickling
                // ClientHello cannot outlive it).
                arm_or_timeout(t, deadline)?;
                t.conn.complete_io(&mut t.sock)?;
            }
            Ok(())
        },
    )?;
    if handshake.is_none() {
        return Ok(());
    }
    // Second first-byte sniff on the decrypted stream (ADR-0266): the TLS
    // stream has no `peek`, so buffer it and `fill_buf` the first decrypted
    // byte without consuming it.
    let mut reader = TlsStream::new(tls);
    let byte = phase(
        &mut reader,
        deadline,
        "bolt: handshake timeout waiting for the first decrypted byte; closing connection",
        |r| r.peek_decrypted().map(|buf| buf.first().copied()),
    )?;
    let Some(Some(byte)) = byte else {
        return Ok(());
    };
    dispatch(
        classify(byte, transport, false),
        reader,
        db,
        config,
        deadline,
    )
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
