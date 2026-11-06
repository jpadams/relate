//! The message vocabulary a script line may name, per side, and the
//! script-notation rendering of a whole message: the Bolt 4.4 names plus
//! the 5.x additions LOGON, LOGOFF and TELEMETRY (ADR-0484). The names are
//! notation, not a version gate — a script may name any of them at any
//! negotiated version, exactly as it may write any `0xNN` tag.

use super::{hex, value};
use crate::common::packstream::Pack;

pub(crate) const HELLO: u8 = 0x01;
pub(crate) const GOODBYE: u8 = 0x02;
pub(crate) const LOGON: u8 = 0x6A;
pub(crate) const SUCCESS: u8 = 0x70;

/// Requests a `C:` line may name.
const CLIENT_MESSAGES: [(&str, u8); 13] = [
    ("HELLO", HELLO),
    ("GOODBYE", GOODBYE),
    ("RESET", 0x0F),
    ("RUN", 0x10),
    ("BEGIN", 0x11),
    ("COMMIT", 0x12),
    ("ROLLBACK", 0x13),
    ("DISCARD", 0x2F),
    ("PULL", 0x3F),
    ("ROUTE", 0x66),
    ("LOGON", LOGON),
    ("LOGOFF", 0x6B),
    ("TELEMETRY", 0x54),
];

/// Responses an `S:` line may name.
const SERVER_MESSAGES: [(&str, u8); 4] = [
    ("SUCCESS", SUCCESS),
    ("RECORD", 0x71),
    ("IGNORED", 0x7E),
    ("FAILURE", 0x7F),
];

/// The side of the conversation a message line belongs to.
#[derive(Clone, Copy)]
pub(crate) enum Role {
    Client,
    Server,
}

impl Role {
    fn messages(self) -> &'static [(&'static str, u8)] {
        match self {
            Role::Client => &CLIENT_MESSAGES,
            Role::Server => &SERVER_MESSAGES,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Role::Client => "client",
            Role::Server => "server",
        }
    }

    /// The tag of a message `name` on this side: a known message name, or
    /// `0xNN` for any tag.
    pub(crate) fn tag(self, name: &str) -> Result<u8, String> {
        if let Some((_, tag)) = self.messages().iter().find(|(known, _)| *known == name) {
            return Ok(*tag);
        }
        if let Some(digits) = name.strip_prefix("0x") {
            return hex::byte(digits).map_err(|e| format!("bad message tag `{name}`: {e}"));
        }
        let known: Vec<&str> = self.messages().iter().map(|(known, _)| *known).collect();
        Err(format!(
            "unknown {} message `{name}` (expected one of {}, or a 0xNN tag)",
            self.label(),
            known.join(", ")
        ))
    }

    /// Render `message` the way a script line writes it: the message name
    /// (or `0xNN`) followed by its fields.
    pub(crate) fn render(self, message: &Pack) -> String {
        let Pack::Struct { tag, fields } = message else {
            return format!("non-structure message {}", value::render(message));
        };
        let name = match self.messages().iter().find(|(_, known)| known == tag) {
            Some((name, _)) => (*name).to_string(),
            None => format!("0x{tag:02X}"),
        };
        fields.iter().fold(name, |line, field| {
            format!("{line} {}", value::render(field))
        })
    }
}
