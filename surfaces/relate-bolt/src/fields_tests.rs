//! Unit tests for the ADR-0342 request-field decoders: the accessor
//! family's absent/default/malformed split and the RUN and PULL/DISCARD
//! requests' one malformed exit each.

use super::*;

#[test]
fn extra_int_or_defaults_only_when_the_key_is_absent() {
    let fields = vec![Pack::Map(vec![
        (Pack::String("n".into()), Pack::Int(3)),
        (Pack::String("qid".into()), Pack::String("0".into())),
    ])];
    assert_eq!(extra_int_or(&fields, 0, "n", -1), Ok(3));
    assert_eq!(extra_int_or(&fields, 0, "absent", -1), Ok(-1));
    assert_eq!(extra_int_or(&[], 0, "n", -1), Ok(-1));
    assert_eq!(
        extra_int_or(&fields, 0, "qid", -1),
        Err("unsupported extra value for qid: String (must be an Int)".into())
    );
}

#[test]
fn extra_str_treats_empty_and_non_strings_as_absent() {
    let fields = vec![Pack::Map(vec![
        (Pack::String("a".into()), Pack::String("".into())),
        (Pack::String("b".into()), Pack::Int(1)),
        (Pack::String("c".into()), Pack::String("x".into())),
    ])];
    assert_eq!(extra_str(&fields, 0, "a"), None);
    assert_eq!(extra_str(&fields, 0, "b"), None);
    assert_eq!(extra_str(&fields, 0, "c"), Some("x"));
    assert_eq!(extra_str(&fields, 0, "absent"), None);
}

#[test]
fn db_name_falls_back_to_the_home_database() {
    assert_eq!(db_name(&[], 2), HOME_DATABASE);
    let empty = Pack::Map(vec![]);
    let named = Pack::Map(vec![(
        Pack::String("db".into()),
        Pack::String("other".into()),
    )]);
    assert_eq!(db_name(&[empty.clone(), empty.clone(), named], 2), "other");
    assert_eq!(db_name(&[empty.clone(), empty.clone(), empty], 2), "neo4j");
}

/// The ADR-0329 `patch_bolt` accessor: only the string entries of a list
/// are names; a non-list value or an extra that is not a map contributes
/// none.
#[test]
fn patch_names_reads_only_a_lists_string_entries() {
    let extra = |value: Pack| vec![Pack::Map(vec![(Pack::String("patch_bolt".into()), value)])];
    assert!(patch_names(&[]).is_empty());
    assert_eq!(
        patch_names(&extra(Pack::List(vec![
            Pack::String("utc".into()),
            Pack::Int(1),
        ]))),
        vec!["utc"]
    );
    assert!(patch_names(&extra(Pack::String("utc".into()))).is_empty());
    assert!(patch_names(&[Pack::Int(1)]).is_empty());
}

/// The malformed-RUN message, from whichever field failed.
fn parse_err(fields: &[Pack]) -> String {
    match RunRequest::parse(fields, Dialect::plain_44()) {
        Ok(_) => panic!("a malformed RUN must fail"),
        Err(message) => message,
    }
}

#[test]
fn run_request_parse_has_one_malformed_exit() {
    let fields = vec![Pack::String("RETURN 1".into()), Pack::Map(vec![])];
    let request = RunRequest::parse(&fields, Dialect::plain_44()).unwrap();
    assert_eq!(request.cypher, "RETURN 1");
    assert!(request.params.is_empty());

    assert_eq!(
        parse_err(&[]),
        "missing RUN statement (RUN statement must be a string)"
    );
    assert_eq!(
        parse_err(&[Pack::Int(1), Pack::Map(vec![])]),
        "unsupported request value for statement: Int (RUN statement must be a string)"
    );
    assert_eq!(
        parse_err(&[Pack::String("RETURN 1".into()), Pack::Int(1)]),
        "unsupported parameter value for $: Int (RUN params must be a map)"
    );
}

/// The malformed-PULL/DISCARD message, from whichever field failed.
fn consume_err(fields: &[Pack]) -> String {
    match ConsumeRequest::parse(fields) {
        Ok(_) => panic!("a malformed PULL/DISCARD must fail"),
        Err(message) => message,
    }
}

/// An absent extra map (or absent keys) is the `-1` default for both
/// fields: qid addresses the most recent RUN and n drains.
#[test]
fn consume_request_parse_defaults_absent_qid_and_n_to_drain() {
    for fields in [vec![], vec![Pack::Map(vec![])]] {
        let request = ConsumeRequest::parse(&fields).unwrap();
        assert_eq!(request.qid, -1);
        assert_eq!(request.cap, None);
        assert_eq!(request.take(7), 7, "an absent cap drains the rows left");
    }
    let request = ConsumeRequest::parse(&[Pack::Map(vec![
        (Pack::String("n".into()), Pack::Int(-1)),
        (Pack::String("qid".into()), Pack::Int(2)),
    ])])
    .unwrap();
    assert_eq!(request.qid, 2);
    assert_eq!(request.cap, None);
}

/// A present non-negative `n` caps the page at `n` rows.
#[test]
fn consume_request_parse_caps_at_a_non_negative_n() {
    let request =
        ConsumeRequest::parse(&[Pack::Map(vec![(Pack::String("n".into()), Pack::Int(3))])])
            .unwrap();
    assert_eq!(request.qid, -1);
    assert_eq!(request.cap, Some(3));
    assert_eq!(
        request.take(5),
        3,
        "a cap under the rows left takes the cap"
    );
}

/// A present `n` outside `-1` and the non-negative Ints is the same
/// malformed request as a non-Int one, never the `-1` drain.
#[test]
fn consume_request_parse_rejects_an_out_of_range_n() {
    for n in [-5, i64::MIN] {
        let fields = vec![Pack::Map(vec![(Pack::String("n".into()), Pack::Int(n))])];
        assert_eq!(
            consume_err(&fields),
            format!("unsupported extra value for n: {n} (must be -1 or a non-negative Int)")
        );
    }
}

/// A huge positive `n` (above `usize::MAX` on a 32-bit target) must decode
/// to the exact cap — saturating only where the conversion cannot hold it
/// — instead of panicking on the target-width conversion, and the take
/// clamp still takes the rows left.
#[test]
fn consume_request_parse_caps_a_huge_n_at_the_rows_left() {
    let fields = vec![Pack::Map(vec![(
        Pack::String("n".into()),
        Pack::Int(i64::MAX),
    )])];
    let request = ConsumeRequest::parse(&fields).unwrap();
    // A 64-bit `usize` holds the huge `n` exactly; a 32-bit one cannot,
    // so the decode saturates it at `usize::MAX` — both still drain the
    // rows left instead of panicking.
    #[cfg(target_pointer_width = "64")]
    assert_eq!(request.cap, Some(i64::MAX as usize));
    #[cfg(target_pointer_width = "32")]
    assert_eq!(request.cap, Some(usize::MAX));
    assert_eq!(
        request.take(2),
        2,
        "a cap above the rows left takes the rows left"
    );
}

/// A present-but-malformed `qid` or `n` fails with the accessor's message,
/// the same one the session forwards as REQUEST_INVALID.
#[test]
fn consume_request_parse_rejects_a_malformed_qid_and_n() {
    let qid = vec![Pack::Map(vec![(
        Pack::String("qid".into()),
        Pack::String("0".into()),
    )])];
    assert_eq!(
        consume_err(&qid),
        "unsupported extra value for qid: String (must be an Int)"
    );
    let n = vec![Pack::Map(vec![(
        Pack::String("n".into()),
        Pack::Float(2.5),
    )])];
    assert_eq!(
        consume_err(&n),
        "unsupported extra value for n: Float (must be an Int)"
    );
}
