//! Shared fixtures and helpers for the acceptance-test modules.

use relate_ontology::{
    parse_ontology_document, to_graph_type_body, OntologyError, Value, PINNED_SCHEMA_URI,
};

/// The Relate-authored, 1.1.0-shape, validation-only example
/// (`neo4j/graph-spec` tag `release-spec/v1.1.0` @
/// `5ff2c20aa7e428e7c563b17f573157293959a009`, ADR-0364 §6, ADR-0487
/// §4).
pub const EXAMPLE_JSON: &str = include_str!("../../schema/example.json");

/// The vendored published schema itself.
pub const VENDORED_SCHEMA: &str = include_str!("../../schema/ontology-graph-spec.schema.json");

/// The vendored schema, parsed: the one read of the fixture every
/// schema-shape assertion shares.
pub fn vendored_schema() -> serde_json::Value {
    serde_json::from_str(VENDORED_SCHEMA).expect("vendored schema is valid JSON")
}

/// The `$defs.Neo4jType.enum` token vocabulary from the vendored schema.
/// The schema is the canonical source this compilation unit can read: the
/// crate's `NEO4J_TYPES` table is `pub(crate)`, out of reach from an
/// integration test.
pub fn neo4j_type_tokens() -> Vec<String> {
    let schema = vendored_schema();
    let tokens = schema["$defs"]["Neo4jType"]["enum"]
        .as_array()
        .expect("vendored schema has a `$defs.Neo4jType.enum` array");
    tokens
        .iter()
        .map(|token| {
            token
                .as_str()
                .expect("every `Neo4jType` enum token is a string")
                .to_owned()
        })
        .collect()
}

/// Assert the gate's version error for `expected_uri`, once for every
/// gate test: the variant, the offending URI, and the message naming the
/// pinned URI as the supported one.
pub fn assert_unsupported_spec_version(err: &OntologyError, expected_uri: &str) {
    let OntologyError::UnsupportedSpecVersion { uri } = err else {
        panic!("expected UnsupportedSpecVersion, got {err}");
    };
    assert_eq!(uri, expected_uri);
    assert!(err.to_string().contains(PINNED_SCHEMA_URI), "{err}");
}

/// The apply-failure scaffold the loud-at-apply tests share: parse a
/// spec-valid fragment (a leading-comma JSON body for [`json_doc`]),
/// derive its graph type, and return the `Mapping` error — panicking
/// when the fragment does not validate or does convert.
pub fn mapping_error(rest: &str) -> OntologyError {
    let doc = parse_ontology_document(json_doc(rest).as_bytes())
        .unwrap_or_else(|e| panic!("the fragment is spec-valid: {e}\n{rest}"));
    let err = to_graph_type_body(&doc).expect_err("the fragment is not convertible");
    assert!(
        matches!(err, OntologyError::Mapping(_)),
        "expected a Mapping error: {err}"
    );
    err
}

/// Wrap a JSON body in a valid spec-shape document header.
pub fn json_doc(rest: &str) -> String {
    format!("{{\"schema\":\"{PINNED_SCHEMA_URI}\",\"id\":\"test\",\"version\":\"1\"{rest}}}")
}

/// Walk `keys` down a value tree, panicking on the first non-object or
/// missing member. Extension payloads have no typed view; assertions read
/// the retained root value tree (ADR-0364 §5.1).
pub fn at<'a>(value: &'a Value, keys: &[&str]) -> &'a Value {
    let mut current = value;
    for (depth, key) in keys.iter().enumerate() {
        current = current
            .as_object()
            .and_then(|obj| obj.get(*key))
            .unwrap_or_else(|| {
                panic!(
                    "no value at '/{}' (not an object at '/{}')",
                    keys[..=depth].join("/"),
                    keys[..depth].join("/")
                )
            });
    }
    current
}
