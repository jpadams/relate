//! WebSocket (RFC 6455) upgrade, Bolt byte-stream adapter, and the WebSocket
//! serve entry point (ADR-0266).
//!
//! [`accept_websocket`] performs the server-side upgrade on any
//! `Read + Write` stream (plain TCP after the `0x47` peek, or a
//! rustls-decrypted stream after the post-TLS re-sniff) and returns a
//! [`BoltWsStream`]. [`serve_websocket`] is the serve entry point wired into
//! `transport::serve_conn` dispatch (the plaintext `0x47` arm and the
//! post-TLS re-sniff): it runs the upgrade, serves Bolt over the adapter
//! through [`crate::protocol::handle_conn`], and then drives the WebSocket
//! close handshake so a client sees a server Close frame before EOF.
//!
//! The upgrade request is read and parsed with tungstenite's own HTTP
//! machinery (`TryParse` / `create_response`) rather than
//! `accept_hdr`, because the contract needs exact control of the
//! rejection responses — `HTTP/1.1 400 Bad Request` for a request that is
//! not a valid WebSocket upgrade, `HTTP/1.1 403 Forbidden` for a rejected
//! Origin — and the `accept_hdr` error path drops the stream before
//! a custom status line can be written.
//!
//! ADR-0233: the framing is sync tungstenite (`WebSocket` over the blocking
//! stream); backpressure is the socket itself — a client that stops reading
//! stalls the connection thread's next flush instead of growing a buffer.

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::time::Instant;

use relate::Database;
use relate_serve::origin::{Origin, OriginFlagValue, OriginMatch};
use relate_serve::ConnectionCap;
use tungstenite::handshake::machine::TryParse;
use tungstenite::handshake::server::{create_response, write_response, Request};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use tungstenite::{http, Bytes, Error as WsError, Message, WebSocket};

use crate::protocol::{arm_phase, handle_conn, is_timeout, ConnConfig, SocketTimeouts};

/// Bound on the HTTP upgrade request (request line plus headers). A request
/// that does not complete within this many bytes is answered with
/// `400 Bad Request`.
pub(crate) const MAX_UPGRADE_REQUEST_BYTES: usize = 8192;

/// A frame declaring more than this is rejected before its payload is
/// buffered (tungstenite `max_frame_size`).
pub const MAX_WS_FRAME_BYTES: usize = 1_048_576;

/// Bound on one reassembled WebSocket message; matches the Bolt 16 MiB
/// message cap so the WS path cannot buffer more than raw TCP accepts
/// (tungstenite `max_message_size`).
pub(crate) const MAX_WS_MESSAGE_BYTES: usize = crate::framing::MAX_MESSAGE_BYTES;

/// The ADR-0266 Origin policy, resolved once at startup from the
/// `--ws-allow-origin` list and the resolved auth mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginPolicy {
    /// Only loopback Origins (`http(s)://localhost[:port]`,
    /// `http(s)://127.0.0.1[:port]`, `http(s)://[::1][:port]`) — the
    /// cross-site WebSocket hijacking guard, the `--auth none` default,
    /// and the never-permissive [`WsConfig::default`]. This is the
    /// surface-specific arm; every other verdict is the shared matcher's,
    /// stored resolved.
    LoopbackOnly,
    /// The authenticated default: loopback Origins (any port) plus the
    /// hosted Neo4j web applications — the Neo4j Browser
    /// (`https://browser.neo4j.io`) and the Neo4j (Aura) Console
    /// (`https://console.neo4j.io`), the shared
    /// `relate_serve::origin::HOSTED_WEB_APP_ORIGINS` list the HTTP
    /// surface's default also consults (ADR-0459). With auth on, a
    /// remote page still cannot act without credentials, so the hosted
    /// apps work out of the box while an arbitrary Origin must be named
    /// explicitly with `--ws-allow-origin`.
    LoopbackAndHosted,
    /// The shared matcher's verdict (`*` or an explicit
    /// `--ws-allow-origin` allowlist). The wildcard allows every Origin,
    /// shape-valid or not (its documented risk); an exact list compares
    /// canonical-to-canonical, so scheme and host case is insignificant,
    /// and an Origin outside the origin shape is never allowed by an exact
    /// list.
    Matched(OriginMatch),
}

impl OriginPolicy {
    /// Resolve the policy from the `--ws-allow-origin` values and the
    /// resolved auth mode: the shared matcher's resolution
    /// ([`OriginMatch::resolve`]) — `*` anywhere selects
    /// [`OriginMatch::Any`], any other non-empty list [`OriginMatch::Exact`]
    /// — and an empty list folds into the auth-dependent default:
    /// [`Self::LoopbackAndHosted`] with auth on (ADR-0459 amends ADR-0266's
    /// any-Origin default), [`Self::LoopbackOnly`] with auth off.
    /// `pub(crate)`: the only caller is [`WsConfig::resolve`], the one
    /// public composition funnel; widen when a second crate resolves a
    /// policy directly.
    pub(crate) fn resolve(allowed_origins: &[OriginFlagValue], auth_enabled: bool) -> Self {
        match OriginMatch::resolve(allowed_origins) {
            Some(matched) => Self::Matched(matched),
            None if auth_enabled => Self::LoopbackAndHosted,
            None => Self::LoopbackOnly,
        }
    }

    /// A present Origin is checked against the policy; the missing-Origin
    /// (non-browser client) case is handled by the caller before this runs.
    /// The shared origin shape is parsed once and every arm projects it:
    /// the loopback arms the serialized-spelling classification
    /// ([`Origin::is_loopback_spelling`]), the hosted arm the canonical
    /// hosted-app match ([`Origin::is_hosted_web_app`]), the matched arm
    /// the shared matcher's canonical verdict ([`OriginMatch::allows`]).
    pub fn allows(&self, origin: &str) -> bool {
        let parsed = Origin::parse(origin);
        match self {
            OriginPolicy::LoopbackOnly => {
                parsed.is_some_and(|parsed| parsed.is_loopback_spelling())
            }
            OriginPolicy::LoopbackAndHosted => parsed
                .is_some_and(|parsed| parsed.is_loopback_spelling() || parsed.is_hosted_web_app()),
            OriginPolicy::Matched(matched) => matched.allows(parsed.as_ref()),
        }
    }
}

/// The Origin excerpt carried by a rejection error: `escape_debug`-escaped
/// with `<`/`>` escaped on top (log/terminal safety) and capped at 128
/// chars, so a hostile Origin cannot spray markup or megabytes into logs.
fn scrub_origin(origin: &str) -> String {
    let mut escaped = String::new();
    for c in origin.chars() {
        match c {
            '<' => escaped.push_str("\\u{3c}"),
            '>' => escaped.push_str("\\u{3e}"),
            _ => escaped.extend(c.escape_debug()),
        }
    }
    escaped.chars().take(128).collect()
}

/// WebSocket serving configuration (ADR-0266): the Origin policy and the
/// opt-in session cap; the framing caps are the
/// [`MAX_UPGRADE_REQUEST_BYTES`], [`MAX_WS_FRAME_BYTES`] and
/// [`MAX_WS_MESSAGE_BYTES`] consts.
#[derive(Debug, Clone)]
pub struct WsConfig {
    /// The resolved Origin policy.
    pub origin_policy: OriginPolicy,
    /// The opt-in bound on concurrent WebSocket sessions (ADR-0459); the
    /// cap's own `0` spelling is [`ConnectionCap`]'s. The accept-time
    /// `--max-connections` still bounds every accepted connection (raw Bolt
    /// and WebSocket alike) independently of this knob.
    pub connection_cap: Arc<ConnectionCap>,
}

impl WsConfig {
    /// The one WebSocket serving policy the standalone server and the CLI's
    /// embedded Bolt surface both build (ADR-0459): the Origin policy from
    /// `--ws-allow-origin` and the auth mode, beside the `--ws-max-connections`
    /// session cap (`0` is unlimited, per [`ConnectionCap`]). One
    /// constructor so the two callers cannot state the cap's
    /// `0`-means-unlimited convention or the Origin default twice. `pub`
    /// because the CLI's Bolt spawner is the second crate that composes the
    /// policy (the composition root, beside its
    /// `OriginAllowlist::from_flag_values` sibling).
    pub fn resolve(
        allowed_origins: &[OriginFlagValue],
        auth_enabled: bool,
        ws_max_connections: usize,
    ) -> Self {
        Self {
            origin_policy: OriginPolicy::resolve(allowed_origins, auth_enabled),
            connection_cap: Arc::new(ConnectionCap::new(ws_max_connections)),
        }
    }
}

impl Default for WsConfig {
    /// Loopback-only and uncapped: the never-permissive, never-capped
    /// default. Permissive policies and a session cap are always explicit
    /// [`Self::resolve`] / operator outcomes.
    fn default() -> Self {
        Self {
            origin_policy: OriginPolicy::LoopbackOnly,
            connection_cap: Arc::new(ConnectionCap::new(0)),
        }
    }
}

/// The tungstenite framing config from the [`MAX_WS_FRAME_BYTES`] and
/// [`MAX_WS_MESSAGE_BYTES`] caps; independent of any [`WsConfig`] field.
pub fn tungstenite_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_frame_size(Some(MAX_WS_FRAME_BYTES))
        .max_message_size(Some(MAX_WS_MESSAGE_BYTES))
}

/// The WebSocket serve entry point (ADR-0266): run the RFC 6455 upgrade on
/// `stream`, serve Bolt over the accepted WebSocket, then drive the
/// WebSocket close handshake so a well-behaved client receives a server
/// Close frame before EOF.
///
/// The upgrade read runs under the connection's ADR-0141 accept-time
/// `deadline`, passed down from `transport::serve_conn` as plain data
/// (`None` disables the bound, e.g. tests). The close handshake is past
/// negotiation, so it gets its own fresh handshake-timeout budget
/// (ADR-0141 covers negotiation only). `ws_config` is shared by reference —
/// the Origin list is never deep-cloned per connection.
///
/// One [`WsConfig::connection_cap`] permit is taken before the upgrade read
/// and held for the whole session (ADR-0459); with a positive
/// `--ws-max-connections`, a session over the cap is accepted then closed,
/// the same policy the accept-time `--max-connections` cap applies, and a
/// `0` cap admits every session.
pub fn serve_websocket<S: Read + Write + SocketTimeouts>(
    mut stream: S,
    ws_config: &WsConfig,
    db: Arc<Database>,
    config: ConnConfig,
    deadline: Option<Instant>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // The opt-in WebSocket session cap: taken before the upgrade read so an
    // over-cap connection costs no handshake parsing, and held until this
    // function returns (the close handshake included). `max == 0` is
    // unlimited, so the default config never refuses.
    let _permit = match ws_config.connection_cap.try_acquire() {
        Some(permit) => permit,
        None => {
            eprintln!(
                "{}",
                relate_serve::over_cap_warning(
                    "bolt",
                    "--ws-max-connections",
                    ws_config.connection_cap.max(),
                )
            );
            return Ok(());
        }
    };
    // The upgrade read is a negotiation phase: re-arm the remaining budget.
    if !arm_phase(&mut stream, deadline)? {
        upgrade_timeout_close();
        return Ok(());
    }
    match accept_websocket(stream, ws_config, deadline) {
        Ok(mut adapter) => {
            // `handle_conn` serves the session on a `&mut` borrow, leaving
            // the adapter here for the close handshake afterwards.
            let shutdown_timeout = config.limits.handshake_timeout;
            let result = handle_conn(&mut adapter, db, config, deadline);
            // The close handshake is past negotiation: a fresh budget (the
            // handshake timeout) bounds it, on read and write alike.
            // Same policy as every other guard install on this branch:
            // propagate — a close handshake whose timeout silently never
            // installed would run unbounded.
            adapter.set_read_timeout(shutdown_timeout)?;
            adapter.set_write_timeout(shutdown_timeout)?;
            if let Err(e) = adapter.shutdown() {
                if is_timeout(&e) {
                    eprintln!("bolt: websocket close handshake timeout; closing connection");
                } else {
                    eprintln!("bolt: websocket close handshake failed: {e}");
                }
            }
            result
        }
        Err(e) => {
            // A socket-deadline expiry during the upgrade read is the
            // ADR-0141 timeout; anything else is a failed upgrade.
            if is_timeout(&e) {
                upgrade_timeout_close();
            } else {
                eprintln!("bolt: websocket upgrade failed: {e}; closing connection");
            }
            Ok(())
        }
    }
}

/// The upgrade-timeout close: one diagnostic for both ways the budget
/// runs out (no budget left at arm time, or the read times out).
fn upgrade_timeout_close() {
    eprintln!("bolt: websocket upgrade timeout; closing connection");
}

/// Perform the RFC 6455 server handshake on `stream` under `config`
/// (ADR-0266) and return the Bolt byte-stream adapter over the accepted
/// WebSocket.
///
/// On rejection the HTTP response is written before the error is returned:
/// `400 Bad Request` when the request is not a valid WebSocket upgrade (or
/// exceeds [`MAX_UPGRADE_REQUEST_BYTES`]), `403 Forbidden` when the Origin
/// policy rejects it. The caller closes the connection on `Err`.
pub fn accept_websocket<S: Read + Write + SocketTimeouts>(
    mut stream: S,
    config: &WsConfig,
    deadline: Option<Instant>,
) -> io::Result<BoltWsStream<S>> {
    // Read the upgrade request, bounded by MAX_UPGRADE_REQUEST_BYTES and by
    // the wall-clock negotiation deadline: each read re-arms the remaining
    // budget, so a client trickling a byte per window cannot outlive it
    // (the per-read timeout alone would grant N x the budget).
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let (request, tail) = loop {
        match Request::try_parse(&buf) {
            Ok(Some((used, request))) => break (request, buf.split_off(used)),
            Ok(None) => {
                if buf.len() >= MAX_UPGRADE_REQUEST_BYTES {
                    write_status(&mut stream, http::StatusCode::BAD_REQUEST)?;
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "websocket upgrade request exceeds MAX_UPGRADE_REQUEST_BYTES",
                    ));
                }
                // Size the read to the remaining room so bytes past the cap
                // are never consumed: a first frame pipelined right behind
                // the request must reach the WebSocket layer intact.
                let room = (MAX_UPGRADE_REQUEST_BYTES - buf.len()).min(1024);
                let mut chunk = [0u8; 1024];
                crate::transport::arm_or_timeout(&mut stream, deadline)?;
                let n = stream.read(&mut chunk[..room])?;
                if n == 0 {
                    // Client went away mid-request; nothing to answer.
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "eof during websocket upgrade request",
                    ));
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) => {
                write_status(&mut stream, http::StatusCode::BAD_REQUEST)?;
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("malformed websocket upgrade request: {e}"),
                ));
            }
        }
    };

    // Validates the upgrade headers (Upgrade, Connection, Sec-WebSocket-Key,
    // Sec-WebSocket-Version) and builds the 101 response.
    let response = match create_response(&request) {
        Ok(response) => response,
        Err(e) => {
            write_status(&mut stream, http::StatusCode::BAD_REQUEST)?;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("not a valid websocket upgrade: {e}"),
            ));
        }
    };

    let origin = match request.headers().get(http::header::ORIGIN) {
        None => None,
        Some(value) => match value.to_str() {
            Ok(origin) => Some(origin),
            // Fail closed: an Origin that is not visible ASCII is rejected,
            // not treated as a missing (non-browser) header.
            Err(_) => {
                write_status(&mut stream, http::StatusCode::FORBIDDEN)?;
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "websocket origin rejected: header is not visible ASCII",
                ));
            }
        },
    };
    // A missing Origin is always allowed, so a rejection always carries a
    // present Origin — logged only as a scrubbed excerpt.
    if let Some(origin) = origin {
        if !config.origin_policy.allows(origin) {
            write_status(&mut stream, http::StatusCode::FORBIDDEN)?;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("websocket origin rejected: {}", scrub_origin(origin)),
            ));
        }
    }

    let mut out = Vec::new();
    write_response(&mut out, &response).map_err(io::Error::other)?;
    stream.write_all(&out)?;
    stream.flush()?;

    let ws = WebSocket::from_partially_read(stream, tail, Role::Server, Some(tungstenite_config()));
    Ok(BoltWsStream::new(ws))
}

/// Write a minimal plain HTTP/1.1 status response (status line plus the
/// empty header block) before any WebSocket framing begins (ADR-0266).
/// `StatusCode`'s `Display` is the full reason phrase (`400 Bad Request`).
fn write_status<S: Write>(stream: &mut S, status: http::StatusCode) -> Result<(), io::Error> {
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes())?;
    stream.flush()
}

/// Bolt byte-stream adapter over a WebSocket (ADR-0266), sync edition
/// (ADR-0233).
///
/// Inbound binary message payloads concatenate into the Bolt byte stream —
/// frame and message boundaries carry no Bolt meaning, and payloads are kept
/// as [`Bytes`] so reads never copy. Each [`Write::write`] sends its buffer
/// as one outbound binary WebSocket message and flushes it (sync
/// tungstenite `send`): a client that stops reading stalls this connection
/// thread on the socket — backpressure is the kernel buffer, not a queue.
/// Pings are answered by tungstenite; the queued pong is flushed before any
/// further input is read, so at most one pong is ever outstanding. A text
/// message closes with close code 1003 (unsupported data); an inbound close
/// frame is answered and ends the read side at a clean EOF. `Message::Frame`
/// is never yielded by the message-level tungstenite config used here.
pub struct BoltWsStream<S: Read + Write> {
    ws: WebSocket<S>,
    /// Inbound payload bytes not yet consumed by `read`.
    read_buf: Bytes,
    /// The read side is finished (close received, stream ended, or error).
    read_done: bool,
    /// A close frame was sent (the 1003 text-frame close, the auto-answered
    /// close reply, or the shutdown close) — `shutdown` must not send a
    /// second one.
    close_sent: bool,
}

impl<S: Read + Write> BoltWsStream<S> {
    pub fn new(ws: WebSocket<S>) -> Self {
        Self {
            ws,
            read_buf: Bytes::new(),
            read_done: false,
            close_sent: false,
        }
    }

    /// The server-side close handshake (ADR-0266): send the normal Close
    /// frame — unless this adapter already sent one (text-frame 1003 or the
    /// close reply) — and flush. Runs under the socket timeouts the caller
    /// armed; an error (including a timeout) ends the connection.
    pub fn shutdown(&mut self) -> io::Result<()> {
        if !self.close_sent {
            self.ws
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Normal,
                    reason: "bye".into(),
                })))
                .map_err(ws_io_error)?;
            self.close_sent = true;
            return Ok(());
        }
        self.flush_queued()
    }

    /// Push out anything tungstenite queued (a pong reply, a close reply).
    fn flush_queued(&mut self) -> io::Result<()> {
        self.ws.flush().map_err(ws_io_error)
    }
}

impl<S: Read + Write + SocketTimeouts> SocketTimeouts for BoltWsStream<S> {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> io::Result<()> {
        self.ws.get_mut().set_read_timeout(dur)
    }
    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> io::Result<()> {
        self.ws.get_mut().set_write_timeout(dur)
    }
}

fn ws_io_error(e: WsError) -> io::Error {
    match e {
        WsError::Io(e) => e,
        other => io::Error::new(io::ErrorKind::InvalidData, other),
    }
}

impl<S: Read + Write> Read for BoltWsStream<S> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if !self.read_buf.is_empty() {
                let n = self.read_buf.len().min(out.len());
                out[..n].copy_from_slice(&self.read_buf[..n]);
                self.read_buf = self.read_buf.split_off(n);
                return Ok(n);
            }
            if self.read_done {
                return Ok(0);
            }
            match self.ws.read() {
                Ok(Message::Binary(data)) => {
                    if data.is_empty() {
                        continue;
                    }
                    self.read_buf = data;
                }
                // tungstenite queued the pong; flush it before reading
                // more, so at most one pong is ever outstanding. A client
                // that never reads stalls this flush — the backpressure
                // that stops a ping flood from growing a buffer.
                Ok(Message::Ping(_)) => self.flush_queued()?,
                Ok(Message::Pong(_)) => continue,
                Ok(Message::Text(_)) => {
                    // Text is unsupported data: close with 1003.
                    if !self.close_sent {
                        self.ws
                            .send(Message::Close(Some(CloseFrame {
                                code: CloseCode::Unsupported,
                                reason: "binary messages only".into(),
                            })))
                            .map_err(ws_io_error)?;
                        self.close_sent = true;
                    }
                    self.read_done = true;
                }
                Ok(Message::Close(_)) => {
                    // tungstenite queued the reply close; flush it, then the
                    // read side ends at a clean EOF.
                    self.flush_queued()?;
                    self.close_sent = true;
                    self.read_done = true;
                }
                // Raw frames only appear with a frame-level config; the
                // message-level config above never yields them.
                Ok(Message::Frame(_)) => continue,
                Err(e) => {
                    self.read_done = true;
                    return Err(ws_io_error(e));
                }
            }
        }
    }
}

impl<S: Read + Write> Write for BoltWsStream<S> {
    /// Send `buf` as one binary WebSocket message (ADR-0266: each outbound
    /// write maps to one binary message). An empty `buf` is a no-op `Ok(0)`.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.ws
            .send(Message::Binary(buf.to_vec().into()))
            .map_err(ws_io_error)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_queued()
    }
}

#[cfg(test)]
#[path = "websocket_tests.rs"]
mod tests;
