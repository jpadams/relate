//! Bolt chunk framing: the 16 MiB message cap and the idle deadline
//! (ADR-0110), NOOP chunks between messages (ADR-0316), and the server's
//! batched reader that frames every message already received (ADR-0317).
//!
//! [`chunk`] is the one chunk-header rule. The server's [`Framer`] and the
//! exact one-message [`read_message`] (test clients, the compat harness)
//! both apply it, so a NOOP means the same thing on both sides.

use std::io::{Error, ErrorKind, Read};
use std::time::{Duration, Instant};

use crate::transport::{arm_or_timeout, SocketTimeouts};

/// The total payload cap of one incoming Bolt message, across all chunks.
pub(crate) const MAX_MESSAGE_BYTES: usize = relate_core::MAX_WIRE_MESSAGE_BYTES;

/// Bytes the server asks for per socket read. One read returns whatever has
/// arrived, up to this, so a pipelined write lands in one batch.
const READ_BYTES: usize = 64 * 1024;

/// The most bytes the connection buffer may hold: one message up to the
/// payload cap, plus one read's worth of bytes behind it (ADR-0317's "one
/// partial message plus one read"). A client that keeps the head message
/// incomplete while streaming — reachable pre-HELLO, since framing runs
/// before the auth gate — fails the connection with `InvalidData` instead
/// of growing the buffer without bound.
const MAX_PENDING_BYTES: usize = MAX_MESSAGE_BYTES + READ_BYTES;

/// What one chunk header means where it was read.
#[derive(Debug, PartialEq, Eq)]
enum Chunk {
    /// An empty chunk where a message would start: a NOOP (ADR-0316).
    Noop,
    /// An empty chunk after a message's body: the message is complete.
    End,
    /// A body chunk of this many bytes.
    Body(usize),
}

/// The one chunk-header rule: header `size`, read after `framed` payload
/// bytes of the current message. A body chunk that would push the message
/// past [`MAX_MESSAGE_BYTES`] fails before its body is allocated or read.
fn chunk(size: u16, framed: usize) -> std::io::Result<Chunk> {
    let n = usize::from(size);
    if n == 0 {
        return Ok(if framed == 0 { Chunk::Noop } else { Chunk::End });
    }
    if n > MAX_MESSAGE_BYTES - framed {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("Bolt message exceeds {MAX_MESSAGE_BYTES} bytes"),
        ));
    }
    Ok(Chunk::Body(n))
}

/// Read exactly one chunked Bolt message, skipping NOOP chunks before it
/// (ADR-0316). It reads no byte past the message, so a caller can read the
/// next one with a fresh call. The client-side reader: test helpers and the
/// compat harness read server replies through it.
pub fn read_message<R: Read>(stream: &mut R) -> Result<Vec<u8>, Error> {
    read_message_counting_noops(stream).map(|(_, payload)| payload)
}

/// [`read_message`] plus the number of NOOP chunks skipped before the
/// message: the shape the keep-alive tests assert (ADR-0312) without
/// re-implementing the chunk rule.
pub(crate) fn read_message_counting_noops<R: Read>(
    stream: &mut R,
) -> Result<(usize, Vec<u8>), Error> {
    let mut noops = 0;
    let mut payload = Vec::new();
    loop {
        let mut header = [0u8; 2];
        stream.read_exact(&mut header)?;
        match chunk(u16::from_be_bytes(header), payload.len())? {
            Chunk::Noop => noops += 1,
            Chunk::End => return Ok((noops, payload)),
            Chunk::Body(n) => {
                let old_len = payload.len();
                payload.resize(old_len + n, 0);
                stream.read_exact(&mut payload[old_len..])?;
            }
        }
    }
}

/// The server's buffered message reader (ADR-0317): bytes are read into a
/// connection buffer, and each [`Framer::next_batch`] returns every
/// complete message in it.
pub(crate) struct Framer {
    /// Bytes received and not yet framed into a message.
    pending: Vec<u8>,
    /// The socket read buffer.
    read: Box<[u8]>,
    /// Where framing of the head message (the message at the front of
    /// `pending`) stopped. Carried across [`Framer::next_batch`] calls so a
    /// trickled message resumes at its last parsed chunk instead of being
    /// re-parsed from its start on every read.
    scan: Scan,
}

/// The parse state of the head message: the offset of its next unparsed
/// chunk header in `pending`, the payload bytes accumulated by its complete
/// chunks (the [`chunk`] cap's running total), and the body ranges of those
/// chunks for the eventual message assembly. All indices are into
/// `pending`; [`Framer::frame_complete`] rebases them by the bytes it
/// drains.
#[derive(Default)]
struct Scan {
    at: usize,
    payload_len: usize,
    bodies: Vec<std::ops::Range<usize>>,
}

/// The complete messages framed from the buffer, and whether a NOOP was
/// skipped while framing them.
struct Framed {
    messages: Vec<Vec<u8>>,
    saw_noop: bool,
}

impl Framer {
    pub(crate) fn new() -> Self {
        Self {
            pending: Vec::new(),
            read: vec![0u8; READ_BYTES].into_boxed_slice(),
            scan: Scan::default(),
        }
    }

    /// Wait for at least one complete message, then return every complete
    /// message already received (ADR-0317). `Ok(None)` is a clean end of
    /// stream at a message boundary.
    ///
    /// `idle` is the ADR-0110 idle timeout: the wait for a message must end
    /// within it, re-armed before every socket read, so a client trickling
    /// a byte per read window cannot hold a message open. A skipped NOOP is
    /// activity and starts a fresh window (ADR-0316). `None` disables it.
    pub(crate) fn next_batch<S: Read + SocketTimeouts + ?Sized>(
        &mut self,
        stream: &mut S,
        idle: Option<Duration>,
    ) -> std::io::Result<Option<Vec<Vec<u8>>>> {
        let mut deadline = idle.map(|d| Instant::now() + d);
        loop {
            let framed = self.frame_complete()?;
            if !framed.messages.is_empty() {
                return Ok(Some(framed.messages));
            }
            if framed.saw_noop {
                deadline = idle.map(|d| Instant::now() + d);
            }
            arm_or_timeout(stream, deadline)?;
            let n = stream.read(&mut self.read)?;
            if n == 0 {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                return Err(ErrorKind::UnexpectedEof.into());
            }
            // The explicit buffer bound: checked before the bytes land, so
            // `pending` never holds more than [`MAX_PENDING_BYTES`]. A
            // client that keeps the head message incomplete while
            // streaming fails here (pre-HELLO included) instead of growing
            // the buffer.
            if self.pending.len() + n > MAX_PENDING_BYTES {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("Bolt framing buffer exceeds {MAX_PENDING_BYTES} bytes"),
                ));
            }
            self.pending.extend_from_slice(&self.read[..n]);
        }
    }

    /// Frame every complete message at the front of the buffer and drop
    /// their bytes (and any NOOPs between them). A partial message stays
    /// buffered; its chunk headers are still checked against the cap, and
    /// its scan state ([`Scan`]) is carried to the next call, so its
    /// complete chunks are parsed once, not again on every read.
    ///
    /// A header that breaks the cap fails the call with `InvalidData`; the
    /// head stays at that header, so a caller that tries again reports it
    /// again instead of mis-framing.
    fn frame_complete(&mut self) -> std::io::Result<Framed> {
        let mut framed = Framed {
            messages: Vec::new(),
            saw_noop: false,
        };
        // Bytes at the front of `pending` already framed or skipped.
        let mut consumed = 0;
        // The head message's carried scan state, rebased on the drain
        // below: `at` and `bodies` are indices into `pending`.
        let mut at = self.scan.at;
        let mut payload_len = self.scan.payload_len;
        let mut bodies = std::mem::take(&mut self.scan.bodies);
        let cap_error = loop {
            let Some(header) = self.pending.get(at..at + 2) else {
                break None;
            };
            let size = u16::from_be_bytes([header[0], header[1]]);
            let ruled = match chunk(size, payload_len) {
                Ok(ruled) => ruled,
                Err(e) => break Some(e),
            };
            match ruled {
                Chunk::Noop => {
                    framed.saw_noop = true;
                    consumed = at + 2;
                    at = consumed;
                    payload_len = 0;
                    bodies.clear();
                }
                Chunk::End => {
                    let mut payload = Vec::with_capacity(payload_len);
                    for body in &bodies {
                        payload.extend_from_slice(&self.pending[body.clone()]);
                    }
                    framed.messages.push(payload);
                    consumed = at + 2;
                    at = consumed;
                    payload_len = 0;
                    bodies.clear();
                }
                Chunk::Body(n) => {
                    let body = at + 2..at + 2 + n;
                    if body.end > self.pending.len() {
                        break None;
                    }
                    at = body.end;
                    payload_len += n;
                    bodies.push(body);
                }
            }
        };
        // Keep the head message's scan state for the next call, rebased by
        // the bytes about to be drained.
        for body in &mut bodies {
            body.start -= consumed;
            body.end -= consumed;
        }
        self.scan.at = at - consumed;
        self.scan.payload_len = payload_len;
        self.scan.bodies = bodies;
        self.pending.drain(..consumed);
        match cap_error {
            Some(e) => Err(e),
            None => Ok(framed),
        }
    }
}

#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;
