//! The captured typed Query API v2 bodies (ADR-0181) the load crate's
//! integration tests share. The fixture `fixtures/typed_wire.json` is owned
//! by this crate and embedded at compile time, so the test crates call
//! [`body_json`] / [`body`] / [`unmappable_element_id`] instead of
//! re-inlining the fixture path and index chain. The fixture is parsed once
//! ([`FIXTURE`]); a lookup that misses panics naming both keys rather than
//! silently serving `null`.

use std::sync::LazyLock;

use serde_json::Value as Json;

/// The captured typed Query API v2 fixture (ADR-0181), embedded at compile
/// time and parsed once.
static FIXTURE: LazyLock<Json> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../fixtures/typed_wire.json"))
        .expect("the typed wire fixture parses")
});

/// One captured body for `image` (a `servers` key such as
/// `"neo4j:2026.09-enterprise"`) and `which` (`"mappable"`,
/// `"mappable_v1_1"`, `"unmappable"`), as the parsed JSON. A missing or
/// misspelled key panics here, naming both keys, instead of serving `null`.
pub fn body_json(image: &str, which: &str) -> Json {
    let body = &FIXTURE["servers"][image][which];
    assert!(
        !body.is_null(),
        "no captured body for servers[{image:?}][{which:?}]"
    );
    body.clone()
}

/// [`body_json`] serialized as the loopback response body.
pub fn body(image: &str, which: &str) -> String {
    body_json(image, which).to_string()
}

/// The `unmappable` body's captured node element id (`u`), read from the
/// fixture the loopback serves — the error must name the exact node it
/// refused to materialize.
pub fn unmappable_element_id(image: &str) -> String {
    body_json(image, "unmappable")["data"]["values"][0][0]["_value"]["_element_id"]
        .as_str()
        .expect("the captured element id")
        .to_string()
}
