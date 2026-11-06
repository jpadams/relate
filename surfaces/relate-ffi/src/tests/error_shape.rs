//! Unit tests for the one error shape (ADR-0502 §2) — the rendering the
//! result channel and the out-param channel share.

use crate::abi::{boundary_error_json, engine_error_json, error_shape_json, panic_message};
use relate::{error_info, Error, ErrorInfo, ErrorStage, INTERNAL_ERROR_INFO, INTERNAL_MESSAGE};

/// The shape renders with the documented keys, in the documented order,
/// and round-trips through a JSON parser — a message carrying quotes,
/// newlines and unicode escapes correctly.
#[test]
fn error_shape_json_renders_the_documented_keys_and_escapes_the_message() {
    let info = ErrorInfo {
        neo4j_code: "Neo.ClientError.Statement.SyntaxError",
        gql_status: "42N06",
        stage: ErrorStage::Compile,
        retryable: false,
    };
    let json = error_shape_json(&info, "quote \" newline\n unicode é");
    // The documented byte order of the shape (ADR-0502 §2).
    assert_eq!(
        json.as_str(),
        "{\"code\":\"Neo.ClientError.Statement.SyntaxError\",\"gql_status\":\"42N06\",\"stage\":\"compile\",\"retryable\":false,\"message\":\"quote \\\" newline\\n unicode é\"}"
    );
    let parsed: serde_json::Value = serde_json::from_str(json.as_str()).unwrap();
    assert_eq!(parsed["code"], "Neo.ClientError.Statement.SyntaxError");
    assert_eq!(parsed["gql_status"], "42N06");
    assert_eq!(parsed["stage"], "compile");
    assert_eq!(parsed["retryable"], false);
    assert_eq!(parsed["message"], "quote \" newline\n unicode é");
    assert_eq!(parsed.as_object().unwrap().len(), 5);
}

/// Every stage renders as its ABI spelling — the exhaustive match cannot
/// compile without one.
#[test]
fn error_shape_stage_spellings() {
    for (stage, expected) in [
        (ErrorStage::Compile, "compile"),
        (ErrorStage::Runtime, "runtime"),
        (ErrorStage::Internal, "internal"),
    ] {
        let info = ErrorInfo {
            neo4j_code: "code",
            gql_status: "gql",
            stage,
            retryable: false,
        };
        assert!(error_shape_json(&info, "m")
            .as_str()
            .contains(&format!("\"stage\":\"{expected}\"")));
    }
}

/// The engine classification renders beside the engine's own message, and
/// the boundary classification beside the boundary's own message — one
/// shape, two sources, both single-sourced off `error_info` (the boundary
/// off the row `relate-error` publishes as `INTERNAL_ERROR_INFO`).
#[test]
fn engine_and_boundary_renderings_derive_from_error_info() {
    // An engine error: the classification and the message both come from
    // the engine (here the retryable write-write conflict).
    let conflict = engine_error_json(&Error::Conflict);
    let parsed: serde_json::Value = serde_json::from_str(conflict.as_str()).unwrap();
    let info = error_info(&Error::Conflict);
    assert_eq!(parsed["code"], info.neo4j_code);
    assert_eq!(parsed["gql_status"], info.gql_status);
    assert_eq!(parsed["stage"], "runtime");
    assert_eq!(parsed["retryable"], true);
    assert_eq!(parsed["message"], "write-write conflict");

    // A boundary failure: the internal-class row of the engine's table,
    // beside the boundary's own message.
    let boundary = boundary_error_json("null pointer");
    let parsed: serde_json::Value = serde_json::from_str(boundary.as_str()).unwrap();
    assert_eq!(parsed["code"], INTERNAL_ERROR_INFO.neo4j_code);
    assert_eq!(parsed["gql_status"], INTERNAL_ERROR_INFO.gql_status);
    assert_eq!(parsed["stage"], "internal");
    assert_eq!(parsed["retryable"], false);
    assert_eq!(parsed["message"], "null pointer");
}

/// The internal class is genericized at this boundary (ADR-0260,
/// OPLANE_REQ-00340359): a storage/IO/locked failure's path, an
/// engine-internal message and a caught panic payload never cross — the
/// client sees the fixed message — while the caller-facing classes keep
/// the engine's Display text (the runtime passthrough is covered above).
#[test]
fn internal_class_messages_are_genericized_not_passed_through() {
    for e in [
        Error::storage("checkpoint /var/lib/relate/secret.rl failed"),
        Error::Locked("/var/lib/relate/secret.rl".into()),
        Error::internal("invariant broken: node table desynced"),
    ] {
        let json = engine_error_json(&e);
        let parsed: serde_json::Value = serde_json::from_str(json.as_str()).unwrap();
        assert_eq!(parsed["stage"], "internal", "{}", json.as_str());
        assert_eq!(parsed["message"], INTERNAL_MESSAGE, "{}", json.as_str());
        assert!(
            !json.as_str().contains("secret.rl") && !json.as_str().contains("invariant"),
            "internal detail must not ride the shape: {}",
            json.as_str()
        );
    }

    // A caught panic is the same policy: the payload never crosses.
    let panicked = panic_message(Box::new("boom in conn op"));
    assert_eq!(panicked, INTERNAL_MESSAGE);
    assert!(!panicked.contains("boom"));
}
