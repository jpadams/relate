//! Bolt version negotiation (ADR-0303, generalized to the two-version set
//! by ADR-0484): the reply follows the client's four `00 RR mm MM`
//! proposals in client order — the first proposal covering a spoken
//! version (4.4 or 5.4) selects that version's `00 00 mm MM` reply, else
//! `00 00 00 00` and a close. Each pinned case runs through the pure
//! [`select_version`] and over a real socket through the full `serve_conn`
//! stack.
use super::*;
use crate::requests::Message;
use crate::testutil::{assert_closes, connect, read_msg, send_struct, spawn_handler, success_map};
use std::io::{Read, Write};
use std::sync::Arc;

/// The wire bytes of the Bolt 4.4 version reply, pinned here independently
/// of the implementation's constant.
const REPLY_4_4: [u8; 4] = [0x00, 0x00, 0x04, 0x04];
/// The wire bytes of the Bolt 5.4 version reply, pinned here independently
/// of the implementation's constant.
const REPLY_5_4: [u8; 4] = [0x00, 0x00, 0x04, 0x05];
/// The wire bytes of the "no common version" reply.
const REPLY_NONE: [u8; 4] = [0x00, 0x00, 0x00, 0x00];

/// One `00 RR mm MM` version proposal from its parts: the offered range
/// floor `range` and the offered version `minor`.`major` — the wire form
/// every pinned negotiation case composes, its version numbers spelled as
/// the versions they name. The reserved first byte is zero here; the one
/// edge probe that flips it spells the flip itself.
fn proposal(range: u8, minor: u8, major: u8) -> [u8; 4] {
    [0x00, range, minor, major]
}

/// The four proposal slots, `proposals` first and the rest zero-padded.
fn padded(proposals: &[[u8; 4]]) -> [[u8; 4]; 4] {
    let mut slots = [[0u8; 4]; 4];
    slots[..proposals.len()].copy_from_slice(proposals);
    slots
}

/// Pin one handshake both ways: [`select_version`] picks `reply` for
/// `proposals`, and a real client sending the magic plus the 16 proposal
/// bytes reads the same `reply`. After a version reply the session is
/// served (HELLO gets SUCCESS); after `00 00 00 00` the server closes
/// (EOF).
fn assert_negotiates(proposals: &[[u8; 4]], reply: [u8; 4]) {
    let slots = padded(proposals);
    let expected = (reply != REPLY_NONE).then_some(reply);
    assert_eq!(
        select_version(&slots).map(BoltVersion::wire),
        expected,
        "select_version for {proposals:02X?}"
    );

    let db = Arc::new(Database::open(":memory:").unwrap());
    let mut client = connect(spawn_handler(db, ConnConfig::default()));
    let mut offer = vec![0x60, 0x60, 0xB0, 0x17];
    offer.extend_from_slice(slots.as_flattened());
    client.write_all(&offer).unwrap();
    client.flush().unwrap();
    let mut answered = [0u8; 4];
    client.read_exact(&mut answered).unwrap();
    assert_eq!(answered, reply, "version reply to {proposals:02X?}");

    if reply == REPLY_NONE {
        assert_closes(
            &mut client,
            &format!("no session may follow the 00000000 reply to {proposals:02X?}"),
        );
    } else {
        // The reply names the spoken version the session now speaks; the
        // HELLO that proves the session started must be valid at it (the
        // 5.x HELLO requires `bolt_agent`, ADR-0484 decision 3).
        let version = BoltVersion::SPOKEN
            .into_iter()
            .find(|v| v.wire() == reply)
            .expect("a spoken version's reply");
        send_struct(
            &mut client,
            Message::Hello.signature(),
            vec![crate::testutil::hello_extra_at(version, vec![])],
        );
        success_map(read_msg(&mut client));
    }
}

#[test]
fn exact_bolt_4_4_proposal_is_selected() {
    assert_negotiates(&[proposal(0, 4, 4)], REPLY_4_4);
}

#[test]
fn range_from_4_4_down_to_4_2_selects_4_4() {
    assert_negotiates(&[proposal(2, 4, 4)], REPLY_4_4);
}

#[test]
fn exact_bolt_5_4_proposal_is_selected() {
    assert_negotiates(&[proposal(0, 4, 5)], REPLY_5_4);
}

#[test]
fn range_from_5_4_down_to_5_2_selects_5_4() {
    assert_negotiates(&[proposal(2, 4, 5)], REPLY_5_4);
}

#[test]
fn bolt_4_3_to_4_0_only_gets_no_version_and_close() {
    assert_negotiates(
        &[
            proposal(0, 3, 4),
            proposal(0, 2, 4),
            proposal(0, 1, 4),
            proposal(0, 0, 4),
        ],
        REPLY_NONE,
    );
}

/// 5.0 is not a spoken version (ADR-0484 offers 4.4 and 5.4 only), and a
/// 5.0-exact proposal covers no 5.4.
#[test]
fn bolt_5_0_only_gets_no_version_and_close() {
    assert_negotiates(&[proposal(0, 0, 5)], REPLY_NONE);
}

#[test]
fn bolt_3_only_gets_no_version_and_close() {
    assert_negotiates(&[proposal(0, 0, 3)], REPLY_NONE);
}

#[test]
fn manifest_proposal_never_matches_and_a_later_4_4_is_selected() {
    assert_negotiates(&[proposal(0, 1, 0xFF), proposal(0, 4, 4)], REPLY_4_4);
}

/// The official Python driver's set (ADR-0484 Context): the manifest
/// marker never matches, the 5.0–5.8 range covers 5.4 and, first in
/// client order, decides the reply.
#[test]
fn the_official_driver_set_negotiates_5_4() {
    assert_negotiates(
        &[
            proposal(0, 1, 0xFF),
            proposal(8, 8, 5),
            proposal(2, 4, 4),
            proposal(0, 0, 3),
        ],
        REPLY_5_4,
    );
}

/// ADR-0484 decision 2: the first proposal covering a spoken version
/// decides — a 5.x range ordered before a 4.4 range answers 5.4.
#[test]
fn bolt_5_range_first_then_4_4_range_selects_5_4() {
    assert_negotiates(&[proposal(8, 8, 5), proposal(2, 4, 4)], REPLY_5_4);
}

/// ADR-0484 decision 2, the other order: a 4.4 range ordered before a
/// 5.x range answers 4.4 — the client's first supported proposal wins,
/// not the server's newest version.
#[test]
fn bolt_4_4_range_first_then_5_range_selects_4_4() {
    assert_negotiates(&[proposal(2, 4, 4), proposal(8, 8, 5)], REPLY_4_4);
}

#[test]
fn range_from_4_5_down_to_4_4_selects_4_4() {
    assert_negotiates(&[proposal(1, 5, 4)], REPLY_4_4);
}

/// The covering rule's edges, on the pure function only.
#[test]
fn covering_rule_edges() {
    let select = |proposals: &[[u8; 4]]| select_version(&padded(proposals)).map(BoltVersion::wire);
    // Zero-filled slots never match.
    assert_eq!(select(&[]), None);
    // A range wider than the minor reaches down to minor 0.
    assert_eq!(select(&[proposal(5, 4, 4)]), Some(REPLY_4_4));
    assert_eq!(select(&[proposal(5, 4, 5)]), Some(REPLY_5_4));
    // A 4.x range must reach down to minor 4: 4.8..4.5 misses, 4.8..4.4
    // covers.
    assert_eq!(select(&[proposal(3, 8, 4)]), None);
    assert_eq!(select(&[proposal(4, 8, 4)]), Some(REPLY_4_4));
    // A 5.x range must reach down to minor 4: 5.8..5.5 misses, 5.8..5.4
    // covers.
    assert_eq!(select(&[proposal(3, 8, 5)]), None);
    assert_eq!(select(&[proposal(4, 8, 5)]), Some(REPLY_5_4));
    // The reserved first byte is not inspected — a proposal with it set
    // still selects.
    for (mut quad, reply) in [
        (proposal(0, 4, 4), REPLY_4_4),
        (proposal(0, 4, 5), REPLY_5_4),
    ] {
        quad[0] = 0xFF;
        assert_eq!(select(&[quad]), Some(reply));
    }
}
