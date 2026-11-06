//! The negotiated Bolt protocol version (ADR-0484 decision 1): a leaf
//! module — no crate imports — that every version consumer imports
//! downward: the negotiated dialect (`dialect`), the request vocabulary
//! (`requests`), the wire codec (`packstream`), the field decoders
//! (`fields`, `param_structs`), the value codec (`value_codec`), the
//! result structures (`value_structs`), the handshake (`protocol`), the
//! session and the test fixtures (`testutil`). Version-gating the
//! vocabulary stays with the vocabulary (`Message::spoken_at` in
//! `requests`, through the derivation below); the session-level facts
//! are named capabilities here, each one exhaustive match.

/// The Bolt protocol versions this server speaks (ADR-0484 decision 1):
/// Bolt 4.4 and Bolt 5.4, no intermediate minor, nothing newer. The
/// negotiated version rides the session's dialect and gates the request
/// vocabulary — the 5.x additions (LOGON, LOGOFF, TELEMETRY) decode only
/// at 5.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoltVersion {
    /// Bolt 4.4 (ADR-0057, ADR-0303).
    V4_4,
    /// Bolt 5.4 (ADR-0484).
    V5_4,
}

impl BoltVersion {
    /// The versions this server speaks (ADR-0484 decision 1): the table
    /// the handshake's selection reads, in spoken order.
    pub(crate) const SPOKEN: [BoltVersion; 2] = [BoltVersion::V4_4, BoltVersion::V5_4];

    /// This version's `00 00 mm MM` wire form: the reply the handshake
    /// sends for it, and the exact form a client proposes for it.
    pub(crate) const fn wire(self) -> [u8; 4] {
        match self {
            BoltVersion::V4_4 => [0x00, 0x00, 0x04, 0x04],
            BoltVersion::V5_4 => [0x00, 0x00, 0x04, 0x05],
        }
    }

    /// This version's spoken name — the spelling the handshake's
    /// no-version diagnostic lists the spoken table in, so a new spoken
    /// version updates the diagnostic with the table, never by hand.
    pub(crate) fn name(self) -> &'static str {
        match self {
            BoltVersion::V4_4 => "Bolt 4.4",
            BoltVersion::V5_4 => "Bolt 5.4",
        }
    }

    /// Whether a connection at this version speaks the 5.x session
    /// protocol (ADR-0484): the HELLO that does not authenticate, the
    /// AUTHENTICATION gate and the 5.x session messages and
    /// notifications keys ride it. The named derivation every
    /// session-level site consults, the request vocabulary's gate
    /// (`Message::spoken_at` in `requests`) included.
    pub(crate) fn speaks_5x(self) -> bool {
        match self {
            BoltVersion::V4_4 => false,
            BoltVersion::V5_4 => true,
        }
    }
}
