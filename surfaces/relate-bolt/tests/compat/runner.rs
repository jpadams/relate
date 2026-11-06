//! Replay a parsed script as the client against a real `relate-bolt`
//! process. Every failure, transport errors included, is reported at the
//! script line whose step failed, so this module drives the fallible wire
//! primitives (`protocol::read_message`, `write_all`) rather than the
//! shared harness's panicking `send` / `read_message`.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::messages::Role;
use super::parser::{Action, Script, Step};
use super::{hex, value, ScriptError};
use crate::common::packstream::decode;
use crate::common::start_server;

/// Read and write guard on the client socket: a server that never answers
/// (or never closes) fails the step instead of hanging the suite.
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Start the script's server, connect, and replay every step in order;
/// the first failing step ends the run.
pub(crate) fn run(script: &Script) -> Result<(), ScriptError> {
    let args: Vec<&str> = script.server_args.iter().map(String::as_str).collect();
    let server = start_server(&args);
    let first = script
        .steps
        .first()
        .expect("the parser rejects a script without steps");
    let mut client = connect(&server.addr).map_err(|e| {
        ScriptError::new(
            &script.path,
            Some(first.at.line),
            format!("cannot connect to {}: {e}", server.addr),
        )
    })?;
    for step in &script.steps {
        execute(&step.action, &mut client).map_err(|actual| failure(script, step, &actual))?;
    }
    Ok(())
}

fn connect(addr: &str) -> std::io::Result<TcpStream> {
    let client = TcpStream::connect(addr)?;
    client.set_read_timeout(Some(IO_TIMEOUT))?;
    client.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(client)
}

fn failure(script: &Script, step: &Step, actual: &str) -> ScriptError {
    let expected = match &step.action {
        Action::Send(bytes) => format!("to send {} bytes", bytes.len()),
        Action::ExpectBytes(bytes) => format!("bytes {}", hex::encode(bytes)),
        Action::ExpectMessage(message) => Role::Server.render(message),
        Action::ExpectClose => "the server to close the connection".to_string(),
    };
    ScriptError::new(
        &script.path,
        Some(step.at.line),
        format!(
            "`{}`\n  expected: {expected}\n  actual:   {actual}",
            step.at.text
        ),
    )
}

/// Run one step; `Err` describes what actually happened.
fn execute(action: &Action, client: &mut TcpStream) -> Result<(), String> {
    match action {
        Action::Send(bytes) => client
            .write_all(bytes)
            .and_then(|()| client.flush())
            .map_err(|e| format!("write failed: {}", describe(&e))),
        Action::ExpectBytes(want) => {
            let (got, read) = read_up_to(client, want.len());
            match read {
                Ok(()) if got == *want => Ok(()),
                Ok(()) => Err(format!("bytes {}", hex::encode(&got))),
                Err(e) => Err(format!(
                    "bytes [{}] then {}",
                    hex::encode(&got),
                    describe(&e)
                )),
            }
        }
        Action::ExpectMessage(want) => {
            let actual = read_message(client)?;
            let decoded = decode(&actual)
                .map_err(|e| format!("undecodable message [{}]: {e}", hex::encode(&actual)))?;
            if value::matches(want, &decoded) {
                Ok(())
            } else {
                Err(Role::Server.render(&decoded))
            }
        }
        Action::ExpectClose => expect_close(client),
    }
}

/// Read up to `n` bytes: what arrived, and `Err` with the reason when that
/// is fewer than `n`.
fn read_up_to(client: &mut TcpStream, n: usize) -> (Vec<u8>, std::io::Result<()>) {
    let mut got = Vec::with_capacity(n);
    // read_to_end keeps the bytes read before an error.
    let read = match client.take(n as u64).read_to_end(&mut got) {
        Ok(k) if k < n => Err(ErrorKind::UnexpectedEof.into()),
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    };
    (got, read)
}

/// One chunked message through the crate's canonical reader.
fn read_message<R: Read>(client: &mut R) -> Result<Vec<u8>, String> {
    relate_bolt::protocol::read_message(client).map_err(|e| describe(&e))
}

/// End of stream (or a reset, the other way a peer's close surfaces) is a
/// pass; anything the server sends instead is decoded into the report.
fn expect_close(client: &mut TcpStream) -> Result<(), String> {
    let (first, read) = read_up_to(client, 1);
    match read {
        Ok(()) => {
            let mut rest = first.as_slice().chain(&mut *client);
            Err(match read_message(&mut rest) {
                Ok(payload) => match decode(&payload) {
                    Ok(message) => Role::Server.render(&message),
                    Err(e) => format!("undecodable message [{}]: {e}", hex::encode(&payload)),
                },
                Err(e) => format!("bytes starting {} then {e}", hex::encode(&first)),
            })
        }
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(describe(&e)),
    }
}

/// An I/O error as a script author reads it.
fn describe(e: &std::io::Error) -> String {
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => {
            format!("nothing within {}s (timed out)", IO_TIMEOUT.as_secs())
        }
        ErrorKind::UnexpectedEof => "connection closed by the server".to_string(),
        ErrorKind::ConnectionReset => "connection reset by the server".to_string(),
        _ => e.to_string(),
    }
}
