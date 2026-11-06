//! ADR-0308 on the HTTP Query API: a statement that references a parameter
//! missing from `parameters` is a 400 with `42N02`, never a `null` row.

use super::tests::{db, dispatch, post, txs, OPEN};
use crate::v2_envelope::tests::body_json;

#[test]
fn a_missing_parameter_is_400_with_42n02() {
    let db = db();
    let reply = dispatch(
        &db,
        &post("neo4j", r#"{"statement": "RETURN $p AS p"}"#),
        OPEN,
        &txs(),
    );
    assert_eq!(reply.status, 400);
    let body = body_json(&reply);
    assert_eq!(body["errors"][0]["code"], "42N02");
    assert_eq!(
        body["errors"][0]["message"],
        "semantic error: ParameterMissing: Expected parameter(s): p"
    );

    // Supplied — even as null — the statement runs.
    let reply = dispatch(
        &db,
        &post(
            "neo4j",
            r#"{"statement": "RETURN $p AS p", "parameters": {"p": null}}"#,
        ),
        OPEN,
        &txs(),
    );
    assert_eq!(reply.status, 200, "{}", body_json(&reply));
}
