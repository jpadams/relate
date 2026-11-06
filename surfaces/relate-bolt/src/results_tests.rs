//! The open-result store (ADR-0311): qid allocation, `take`'s qid
//! addressing and `clear`'s reset.

use super::*;

/// A failed body holds no rows, so the byte budget is untouched.
fn failed() -> Body {
    Body::Failed(relate::Error::internal("test"))
}

/// `open` hands back the qid it reported; `take` addresses the result
/// through that qid, not the most recent.
#[test]
fn the_open_qid_addresses_the_result_it_stored() {
    let mut results = OpenResults::default();
    let first = results
        .open(
            failed(),
            0,
            "RETURN 1".into(),
            NotificationConfig::default(),
        )
        .unwrap();
    let second = results
        .open(
            failed(),
            0,
            "RETURN 1".into(),
            NotificationConfig::default(),
        )
        .unwrap();
    assert_eq!((first, second), (0, 1));
    assert_eq!(results.take(first).unwrap().qid, first);
    assert_eq!(results.take(second).unwrap().qid, second);
}

/// `qid = -1` addresses the most recent RUN's result through `find`; a
/// taken result is no longer found.
#[test]
fn minus_one_addresses_the_most_recent_result() {
    let mut results = OpenResults::default();
    results
        .open(
            failed(),
            0,
            "RETURN 1".into(),
            NotificationConfig::default(),
        )
        .unwrap();
    let last = results
        .open(
            failed(),
            0,
            "RETURN 1".into(),
            NotificationConfig::default(),
        )
        .unwrap();
    let taken = results.take(-1).unwrap();
    assert_eq!(taken.qid, last);
    assert_eq!(results.take(0).unwrap().qid, 0);
    assert!(results.take(-1).is_none());
}

/// `clear` drops every open result and restarts qids at 0.
#[test]
fn clear_drops_every_result_and_restarts_qids() {
    let mut results = OpenResults::default();
    results
        .open(
            failed(),
            0,
            "RETURN 1".into(),
            NotificationConfig::default(),
        )
        .unwrap();
    assert!(!results.is_empty());
    results.clear();
    assert!(results.is_empty());
    // No RUN has been handed a qid since the clear, so `-1` addresses
    // nothing: the most-recent qid is derived from the counter.
    assert!(results.take(-1).is_none());
    assert_eq!(
        results
            .open(
                failed(),
                0,
                "RETURN 1".into(),
                NotificationConfig::default()
            )
            .unwrap(),
        0
    );
}

/// The reservation counts the retained RUN statement text (ADR-0357)
/// beside the rows, in both arms: a statement longer than the whole budget
/// does not open, and a failed body's counted bytes are exactly its
/// statement.
#[test]
fn the_retained_statement_counts_against_the_cap() {
    let mut results = OpenResults::default();
    // 12 statement bytes against a 4-byte budget: refused.
    assert!(results
        .open(
            failed(),
            4,
            "RETURN 12345".into(),
            NotificationConfig::default()
        )
        .is_err());
    // 8 statement bytes under a 12-byte budget: opens, and the counted
    // bytes are the statement (a failed body retains no rows).
    let qid = results
        .open(
            failed(),
            12,
            "RETURN 1".into(),
            NotificationConfig::default(),
        )
        .unwrap();
    let taken = results.take(qid).unwrap();
    assert_eq!(taken.bytes, "RETURN 1".len() as u64);
}
