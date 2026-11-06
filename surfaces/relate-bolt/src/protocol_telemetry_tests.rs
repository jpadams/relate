//! TELEMETRY (ADR-0484 decision 5): the opted-out no-op. The HELLO
//! SUCCESS advertises `telemetry.enabled: false` beside the receive-timeout
//! hint, every in-range `api` Int answers SUCCESS `{}` with nothing
//! recorded, and the malformed `api` forms answer their pinned codes and
//! texts. The HELLO metadata's full shape is pinned by the logon
//! module (ADR-0484 decision 3's HELLO bullet).

use crate::failures::REQUEST_INVALID_CODE;
use crate::packstream::Pack;
use crate::requests::Message;
use crate::testutil::{
    assert_ignored, assert_ignored_in_failed_state_then_recovers, connected_client_at, failure_map,
    field, read_msg, send_run, send_telemetry, success_map,
};
use crate::version::BoltVersion;

/// The server collects nothing (ADR-0484 decision 5): every driver-API
/// `api` Int the manual defines — 0 through 3 — answers SUCCESS `{}`, and
/// the session stays usable afterwards.
#[test]
fn telemetry_answers_success_for_the_api_ints() {
    let mut client = connected_client_at(BoltVersion::V5_4);
    for api in 0..=3 {
        send_telemetry(&mut client, Some(Pack::Int(api)));
        assert_eq!(success_map(read_msg(&mut client)), vec![], "api {api}");
    }
    send_run(&mut client, "RETURN 1 AS n");
    success_map(read_msg(&mut client));
    crate::testutil::drain_all(&mut client, Message::Pull);
}

/// The malformed `api` forms — the probed out-of-range Int and non-Int,
/// and this server's missing-field spelling (unpinned by the probe) —
/// answer their pinned failure rows, one row per form in the logon
/// module's table shape, and each leaves the failed state: RUN is
/// IGNORED after every row, the aftermath asserted once for the family.
#[test]
fn malformed_telemetry_answers_its_pinned_failures_and_fails_the_session() {
    for (api, code, expected) in [
        (
            Some(Pack::Int(9001)),
            "Neo.DatabaseError.General.UnknownError",
            "org.neo4j.packstream.error.reader.PackstreamReaderException: Unknown driver interface \
             type 9001",
        ),
        (
            Some(Pack::String("0".into())),
            REQUEST_INVALID_CODE,
            "Unexpected type: Expected INT but got STRING",
        ),
        (
            None,
            REQUEST_INVALID_CODE,
            "missing TELEMETRY api (TELEMETRY api must be an Int)",
        ),
    ] {
        let mut client = connected_client_at(BoltVersion::V5_4);
        send_telemetry(&mut client, api.clone());
        let failure = failure_map(read_msg(&mut client));
        assert_eq!(field(&failure, "code"), code, "api {api:?}");
        assert_eq!(field(&failure, "message"), expected, "api {api:?}");
        send_run(&mut client, "RETURN 1 AS n");
        assert_ignored(&mut client, "the failure leaves the failed state");
    }
}

/// The failed state IGNOREs TELEMETRY like the work messages (the same
/// rule this feature's contract pins for LOGOFF): the shared
/// ignore-then-recover exchange, which the logon module drives over
/// LOGOFF.
#[test]
fn telemetry_in_the_failed_state_answers_ignored() {
    let mut client = connected_client_at(BoltVersion::V5_4);
    assert_ignored_in_failed_state_then_recovers(
        &mut client,
        Message::Telemetry,
        vec![Pack::Int(0)],
    );
}
