//! Chunk framing: the chunk rule, NOOP skipping (ADR-0316), the batched
//! server reader (ADR-0317), and both behaviors end to end.

use std::collections::VecDeque;
use std::io::Write;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;

use super::*;
use crate::packstream::{chunk_message, decode, struct_msg, Pack};
use crate::requests::Message;
use crate::testutil::{
    assert_closes, assert_ignored, connect, connected_client, handshake, map, read_msg,
    read_records, send_pull, send_run, send_struct, spawn_handler, success_map,
};

/// Body ranges as `(start, end)` pairs, for comparing [`Framer`] scan state
/// without a single-range array literal (`clippy::single_range_in_vec_init`).
fn bodies(pairs: &[(usize, usize)]) -> Vec<std::ops::Range<usize>> {
    pairs.iter().map(|&(start, end)| start..end).collect()
}

/// A chunked message from raw chunk bodies plus the `00 00` terminator.
fn chunked(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(&(part.len() as u16).to_be_bytes());
        out.extend_from_slice(part);
    }
    out.extend_from_slice(&0u16.to_be_bytes());
    out
}

/// A stream that returns one queued slice per `read` call (the socket
/// delivering bytes in pieces), then end of stream.
struct Reads(VecDeque<Vec<u8>>);

impl Read for Reads {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some(mut next) = self.0.pop_front() else {
            return Ok(0);
        };
        let n = next.len().min(buf.len());
        buf[..n].copy_from_slice(&next[..n]);
        if n < next.len() {
            self.0.push_front(next.split_off(n));
        }
        Ok(n)
    }
}

impl SocketTimeouts for Reads {
    fn set_read_timeout(&mut self, _dur: Option<Duration>) -> std::io::Result<()> {
        Ok(())
    }
    fn set_write_timeout(&mut self, _dur: Option<Duration>) -> std::io::Result<()> {
        Ok(())
    }
}

fn reads(pieces: Vec<Vec<u8>>) -> Reads {
    Reads(pieces.into())
}

/// The endless 1-byte-chunk wire pattern (`00 01 41` repeating): a message
/// that never reaches its `00 00` terminator. The one source of the pattern,
/// shared by [`EndlessChunks`] and the end-to-end writer.
fn endless_chunk_bytes(len: usize) -> Vec<u8> {
    (0..len)
        .map(|at| match at % 3 {
            0 => 0x00,
            1 => 0x01,
            _ => 0x41,
        })
        .collect()
}

/// An endless stream of 1-byte chunks (`00 01 41` repeating): a message
/// that never reaches its `00 00` terminator.
#[derive(Default)]
struct EndlessChunks {
    at: usize,
}

impl Read for EndlessChunks {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let pattern = endless_chunk_bytes(self.at + buf.len());
        buf.copy_from_slice(&pattern[self.at..]);
        self.at += buf.len();
        Ok(buf.len())
    }
}

impl SocketTimeouts for EndlessChunks {
    fn set_read_timeout(&mut self, _dur: Option<Duration>) -> std::io::Result<()> {
        Ok(())
    }
    fn set_write_timeout(&mut self, _dur: Option<Duration>) -> std::io::Result<()> {
        Ok(())
    }
}

fn signatures(batch: &[Vec<u8>]) -> Vec<u8> {
    batch
        .iter()
        .map(|payload| match decode(payload).unwrap() {
            Pack::Struct { tag, .. } => tag,
            other => panic!("not a struct: {other:?}"),
        })
        .collect()
}

#[test]
fn the_chunk_rule_tells_noop_end_and_body_and_enforces_the_cap() {
    assert_eq!(chunk(0, 0).unwrap(), Chunk::Noop);
    assert_eq!(chunk(0, 5).unwrap(), Chunk::End);
    assert_eq!(chunk(7, 0).unwrap(), Chunk::Body(7));
    assert_eq!(
        chunk(u16::MAX, MAX_MESSAGE_BYTES - 65535).unwrap(),
        Chunk::Body(65535)
    );
    assert_eq!(
        chunk(1, MAX_MESSAGE_BYTES).unwrap_err().kind(),
        ErrorKind::InvalidData
    );
}

/// ADR-0316: the client-side reader skips NOOPs before a message and
/// reads nothing past it.
#[test]
fn read_message_skips_noops_and_stops_at_the_message_end() {
    let mut bytes = vec![0, 0, 0, 0];
    bytes.extend(struct_msg(0x0F, vec![]));
    bytes.extend([0, 0]);
    bytes.extend(struct_msg(0x02, vec![]));
    let mut input = &bytes[..];
    assert_eq!(signatures(&[read_message(&mut input).unwrap()]), [0x0F]);
    assert_eq!(signatures(&[read_message(&mut input).unwrap()]), [0x02]);
}

/// ADR-0317: one read carrying several messages and NOOPs is one batch;
/// a partial message stays buffered for the next batch.
#[test]
fn a_batch_is_every_complete_message_already_received() {
    let mut first = struct_msg(0x10, vec![Pack::String("RETURN 1".into())]);
    first.extend([0, 0]);
    first.extend(struct_msg(0x3F, vec![map(vec![])]));
    first.extend(struct_msg(0x0F, vec![]));
    let partial = struct_msg(0x02, vec![]);
    first.extend(&partial[..2]);
    let mut framer = Framer::new();
    let mut stream = reads(vec![first, partial[2..].to_vec()]);
    let batch = framer.next_batch(&mut stream, None).unwrap().unwrap();
    assert_eq!(signatures(&batch), [0x10, 0x3F, 0x0F]);
    let batch = framer.next_batch(&mut stream, None).unwrap().unwrap();
    assert_eq!(signatures(&batch), [0x02]);
    assert!(framer.next_batch(&mut stream, None).unwrap().is_none());
}

/// A message split across reads is framed once its last chunk arrives;
/// end of stream mid-message is an error, at a boundary it is clean.
#[test]
fn a_message_split_across_reads_waits_for_its_end() {
    let message = struct_msg(0x0F, vec![]);
    let mut framer = Framer::new();
    let mut stream = reads(message.iter().map(|byte| vec![*byte]).collect());
    let batch = framer.next_batch(&mut stream, None).unwrap().unwrap();
    assert_eq!(signatures(&batch), [0x0F]);
    assert!(framer.next_batch(&mut stream, None).unwrap().is_none());

    let mut stream = reads(vec![message[..3].to_vec()]);
    let err = Framer::new().next_batch(&mut stream, None).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
}

/// The cap is checked at the chunk header: a header that crosses 16 MiB
/// fails before its body arrives.
#[test]
fn an_oversized_chunk_header_fails_before_its_body() {
    let mut header_only = Vec::new();
    for _ in 0..256 {
        header_only.extend(65535u16.to_be_bytes());
        header_only.extend(vec![0u8; 65535]);
    }
    header_only.extend(257u16.to_be_bytes());
    let mut stream = reads(vec![header_only]);
    let err = Framer::new().next_batch(&mut stream, None).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);
}

/// ADR-0317's buffer bound: a message that never completes while bytes keep
/// arriving fails the connection with `InvalidData` once the buffer passes
/// one message plus one read, instead of growing without bound.
#[test]
fn a_partial_message_past_the_buffer_bound_fails() {
    let mut framer = Framer::new();
    let err = framer
        .next_batch(&mut EndlessChunks::default(), None)
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("framing buffer"),
        "the buffer bound, not the message cap, must have failed: {err}"
    );
    assert!(
        framer.pending.len() > MAX_MESSAGE_BYTES,
        "the buffer bound trips past one message, not at it"
    );
    assert!(
        framer.pending.len() <= MAX_PENDING_BYTES,
        "the buffer is never extended past the bound"
    );
}

/// ADR-0317: the head message's scan cursor is carried across calls — the
/// already-parsed complete chunks are not re-parsed, and a partial body
/// resumes where the previous call stopped.
#[test]
fn a_trickled_message_carries_its_scan_cursor() {
    let message = chunked(&[b"ab", b"cd", b"ef"]);
    let mut framer = Framer::new();
    // One complete chunk and a partial second body: the cursor holds that
    // body and its accumulated payload, and waits on the second header.
    framer.pending.extend_from_slice(&message[..7]);
    let framed = framer.frame_complete().unwrap();
    assert!(framed.messages.is_empty());
    assert_eq!(framer.scan.at, 4);
    assert_eq!(framer.scan.payload_len, 2);
    assert_eq!(framer.scan.bodies, bodies(&[(2, 4)]));
    // The body completes and the third header is parsed: the cursor moves
    // past both without re-parsing the first chunk.
    framer.pending.extend_from_slice(&message[7..12]);
    let framed = framer.frame_complete().unwrap();
    assert!(framed.messages.is_empty());
    assert_eq!(framer.scan.at, 12);
    assert_eq!(framer.scan.payload_len, 6);
    assert_eq!(framer.scan.bodies, bodies(&[(2, 4), (6, 8), (10, 12)]));
    // The terminator assembles the payload from the carried ranges, and
    // the drained buffer leaves the cursor at a fresh message.
    framer.pending.extend_from_slice(&message[12..]);
    let framed = framer.frame_complete().unwrap();
    assert_eq!(framed.messages, [b"abcdef".to_vec()]);
    assert_eq!(framer.scan.at, 0);
    assert_eq!(framer.scan.payload_len, 0);
    assert!(framer.scan.bodies.is_empty());
    assert!(framer.pending.is_empty());
}

/// ADR-0317: a multi-chunk message trickled one byte per read frames once
/// its terminator arrives; the carried cursor makes the trickle linear.
#[test]
fn a_trickled_multi_chunk_message_frames_once() {
    let payload = vec![0x5A; 70_000];
    let message = chunk_message(&payload);
    let mut framer = Framer::new();
    let mut stream = reads(message.iter().map(|byte| vec![*byte]).collect());
    let batch = framer.next_batch(&mut stream, None).unwrap().unwrap();
    assert_eq!(batch, [payload]);
    assert_eq!(framer.scan.at, 0);
    assert!(framer.scan.bodies.is_empty());
    assert!(framer.next_batch(&mut stream, None).unwrap().is_none());
}

/// Run `write` over a fresh client connection to an ephemeral loopback
/// listener, then close the client's write side; returns the accepted
/// server socket and the writer's join handle.
fn loopback_server(
    write: impl FnOnce(&mut TcpStream) + Send + 'static,
) -> (TcpStream, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let writer = std::thread::spawn(move || {
        let mut client = TcpStream::connect(addr).unwrap();
        write(&mut client);
        client.shutdown(Shutdown::Write).unwrap();
    });
    let (server, _) = listener.accept().unwrap();
    (server, writer)
}

/// A message of exactly [`MAX_MESSAGE_BYTES`] is accepted, and the reader
/// stops at its end: the next message is framed on a fresh call.
#[test]
fn exact_message_limit_is_accepted() {
    let (mut server, writer) = loopback_server(|client| {
        // chunk_message emits 257 chunks and a trailing zero-length
        // terminator, followed here by another complete message.
        client
            .write_all(&chunk_message(&vec![0; MAX_MESSAGE_BYTES]))
            .unwrap();
        client.write_all(&chunk_message(&[0xC0])).unwrap();
    });
    let payload = read_message(&mut server).unwrap();
    assert_eq!(payload.len(), MAX_MESSAGE_BYTES);
    assert!(payload.iter().all(|byte| *byte == 0));
    assert_eq!(read_message(&mut server).unwrap(), [0xC0]);
    writer.join().unwrap();
}

/// End of stream mid-message is `UnexpectedEof`.
#[test]
fn truncated_chunk_returns_io_error() {
    let (mut server, writer) = loopback_server(|client| {
        client.write_all(&[0, 2, 0xC0, 0, 4, 0xC0]).unwrap();
    });
    assert_eq!(
        read_message(&mut server).unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    writer.join().unwrap();
}

/// ADR-0317's buffer bound end to end, pre-HELLO: a client that keeps one
/// message incomplete while streaming forever is disconnected by the
/// server instead of growing its buffer without bound.
#[test]
fn an_over_bound_partial_message_closes_the_connection() {
    let db = Arc::new(relate::Database::open(":memory:").unwrap());
    let addr = spawn_handler(db, Default::default());
    let mut client = connect(addr);
    handshake(&mut client);
    // The endless 1-byte-chunk message, in 3-byte-aligned writes so the
    // pattern continues across them.
    let endless = endless_chunk_bytes(3 * 4096);
    client
        .set_write_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    // Bounded so a regression fails the read below instead of writing
    // forever.
    let mut written = 0;
    while written <= 3 * MAX_PENDING_BYTES && client.write_all(&endless).is_ok() {
        written += endless.len();
    }
    let mut buf = [0u8; 1];
    let closed = match client.read(&mut buf) {
        Ok(n) => n == 0,
        Err(e) => e.kind() == ErrorKind::ConnectionReset,
    };
    assert!(closed, "the server must close an over-bound connection");
}

/// F-29 / ADR-0316 end to end: a client NOOP between RUN and PULL is
/// skipped and the PULL is answered.
#[test]
fn a_client_noop_between_run_and_pull_is_skipped() {
    let mut client = connected_client();
    send_run(&mut client, "RETURN 1 AS x");
    success_map(read_msg(&mut client));
    client.write_all(&[0, 0, 0, 0]).unwrap();
    send_pull(&mut client, Message::Pull, vec![("n", Pack::Int(-1))]);
    let (records, summary) = read_records(&mut client);
    assert_eq!(records, vec![vec![Pack::Int(1)]]);
    success_map(summary);
}

/// F-27 / ADR-0317 end to end: RUN, PULL, RESET, RUN, PULL in one write
/// answer IGNORED, IGNORED, SUCCESS, then the second RUN normally.
#[test]
fn reset_answers_ignored_for_the_work_queued_ahead_of_it() {
    let mut client = connected_client();
    let mut pipelined = Vec::new();
    for message in [
        struct_msg(
            0x10,
            vec![
                Pack::String("UNWIND range(1, 3) AS x RETURN x".into()),
                map(vec![]),
                map(vec![]),
            ],
        ),
        struct_msg(0x3F, vec![map(vec![("n", Pack::Int(-1))])]),
        struct_msg(0x0F, vec![]),
        struct_msg(
            0x10,
            vec![
                Pack::String("RETURN 2 AS y".into()),
                map(vec![]),
                map(vec![]),
            ],
        ),
        struct_msg(0x3F, vec![map(vec![("n", Pack::Int(-1))])]),
    ] {
        pipelined.extend(message);
    }
    client.write_all(&pipelined).unwrap();
    assert_ignored(&mut client, "a RUN queued ahead of the RESET");
    assert_ignored(&mut client, "a PULL queued ahead of the RESET");
    assert!(success_map(read_msg(&mut client)).is_empty());
    let run = Pack::Map(success_map(read_msg(&mut client)));
    assert_eq!(
        run.get("fields"),
        Some(&Pack::List(vec![Pack::String("y".into())]))
    );
    let (records, summary) = read_records(&mut client);
    assert_eq!(records, vec![vec![Pack::Int(2)]]);
    success_map(summary);
}

/// ADR-0317: an explicit transaction open when the queued RESET arrives is
/// rolled back by it, and the work queued ahead of it never runs.
#[test]
fn a_queued_reset_rolls_back_without_running_the_work_ahead_of_it() {
    let db = Arc::new(relate::Database::open(":memory:").unwrap());
    let mut client = crate::testutil::connected_client_with(Arc::clone(&db), Default::default());
    send_struct(&mut client, 0x11, vec![map(vec![])]);
    success_map(read_msg(&mut client));
    let mut pipelined = struct_msg(
        0x10,
        vec![
            Pack::String("CREATE (:Queued)".into()),
            map(vec![]),
            map(vec![]),
        ],
    );
    pipelined.extend(struct_msg(0x12, vec![]));
    pipelined.extend(struct_msg(0x0F, vec![]));
    client.write_all(&pipelined).unwrap();
    for _ in 0..2 {
        assert_ignored(&mut client, "work queued ahead of the RESET");
    }
    success_map(read_msg(&mut client));
    let count = db
        .connect()
        .unwrap()
        .query("MATCH (n:Queued) RETURN count(n) AS c")
        .unwrap();
    assert_eq!(count.rows, vec![vec![relate::Value::Int(0)]]);
}

/// ADR-0317: only the decodable prefix can hold the RESET. A truncated
/// message with a RESET signature bounds the prefix, so the RUN before it
/// is not queued ahead of the valid RESET after it: the RUN runs, then the
/// connection closes on the undecodable message.
#[test]
fn an_undecodable_message_bounds_the_reset_prefix() {
    let mut client = connected_client();
    let mut pipelined = struct_msg(
        0x10,
        vec![
            Pack::String("RETURN 1 AS x".into()),
            map(vec![]),
            map(vec![]),
        ],
    );
    // A struct marker declaring two fields and the RESET tag, then no field
    // bytes: a RESET signature that cannot decode.
    pipelined.extend(chunk_message(&[0xB2, 0x0F]));
    pipelined.extend(struct_msg(0x0F, vec![]));
    client.write_all(&pipelined).unwrap();
    let run = Pack::Map(success_map(read_msg(&mut client)));
    assert_eq!(
        run.get("fields"),
        Some(&Pack::List(vec![Pack::String("x".into())])),
        "the RUN before the undecodable message is not ahead of the RESET after it"
    );
    assert_closes(&mut client, "the undecodable message closes the connection");
}

/// ADR-0317: a message after the first undecodable one is never reached.
/// The RESET ahead of it runs, and the RUN queued behind it is never
/// answered.
#[test]
fn a_message_after_an_undecodable_one_is_never_reached() {
    let mut client = connected_client();
    let mut pipelined = struct_msg(
        0x10,
        vec![
            Pack::String("RETURN 1 AS x".into()),
            map(vec![]),
            map(vec![]),
        ],
    );
    pipelined.extend(struct_msg(0x0F, vec![]));
    pipelined.extend(chunk_message(&[0xB2, 0x0F]));
    pipelined.extend(struct_msg(
        0x10,
        vec![
            Pack::String("RETURN 2 AS y".into()),
            map(vec![]),
            map(vec![]),
        ],
    ));
    client.write_all(&pipelined).unwrap();
    assert_ignored(&mut client, "the RUN queued ahead of the RESET");
    success_map(read_msg(&mut client));
    assert_closes(
        &mut client,
        "the RUN after the undecodable message is never answered",
    );
}
