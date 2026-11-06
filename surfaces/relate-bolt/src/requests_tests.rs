//! The request vocabulary's round trip (ADR-0315): every signature maps
//! back to its message at the version that speaks it, and nothing else is
//! a request.

use super::*;

/// The request vocabulary: every signature maps back to its message at
/// the version that speaks it, and nothing else is a request.
#[test]
fn signatures_round_trip_and_nothing_else_is_a_request() {
    for version in BoltVersion::SPOKEN {
        let spoken = match version {
            BoltVersion::V4_4 => 10,
            BoltVersion::V5_4 => 13,
        };
        let mut known = 0;
        for signature in 0..=u8::MAX {
            if let Some(message) = Message::from_signature(signature, version) {
                assert_eq!(message.signature(), signature);
                known += 1;
            }
        }
        assert_eq!(known, spoken, "the vocabulary at {version:?}");
        assert_eq!(Message::from_signature(0x0E, version), None);
    }
}

/// The 5.x additions decode at 5.4 only (ADR-0484): LOGON `0x6A`, LOGOFF
/// `0x6B` and TELEMETRY `0x54` are requests at 5.4, and at 4.4 are the
/// same fatal unknowns as any other unlisted signature (ADR-0315).
#[test]
fn the_5x_additions_decode_at_5_4_only() {
    for (signature, message) in [
        (0x6A, Message::Logon),
        (0x6B, Message::Logoff),
        (0x54, Message::Telemetry),
    ] {
        assert_eq!(
            Message::from_signature(signature, BoltVersion::V4_4),
            None,
            "{signature:#04x} at 4.4"
        );
        assert_eq!(
            Message::from_signature(signature, BoltVersion::V5_4),
            Some(message),
            "{signature:#04x} at 5.4"
        );
    }
}
