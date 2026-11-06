//! Type-fidelity round-trip through `load_streaming` (ADR-0181): pull each
//! captured typed Query API v2 `mappable` body through the
//! `relate_test_support::loopback` fixture and assert every property
//! keeps its exact engine type and value — `valueType()` spells the type
//! back, temporal and point values compare equal to the matching Cypher
//! constructor, and the types survive the pull's checkpoint and a reopen.
//! The `unmappable` body fails the pull with `LoadError::TypedWire` and
//! leaves the destination empty (ADR-0181 item 7).
//!
//! The bodies are the verbatim captures documented in
//! `fixtures/typed_wire.json` (seed: `fixtures/typed_wire_seed.cypher`).

mod common;

use std::collections::BTreeSet;

use common::{body, body_json, unmappable_element_id};
use relate::{Database, Value};
use relate_load::{load_streaming, DecodeError, LoadError};
use relate_test_support::loopback::{loopback_opts, spawn_single_response};
use serde_json::Value as Json;

const IMAGE_2026_09: &str = "neo4j:2026.09-enterprise";
const IMAGE_5_26: &str = "neo4j:5.26-enterprise";

/// The shape query both captured `mappable` bodies answer (the fixture's
/// `seed_queries.mappable`).
const SHAPE_QUERY: &str = "MATCH (n:TypedSample)-[r:TYPED_REL]->(m) RETURN n, r, m";

/// Pull the captured `mappable` body for `image` into a fresh on-disk
/// `Database`. The loopback serves one body per connection, so each pull
/// spawns its own server. The returned tempdir guard keeps the store alive.
fn pull_mappable(image: &str) -> (tempfile::TempDir, Database) {
    let (server, port) = spawn_single_response("200 OK", body(image, "mappable"));
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(dir.path().to_str().expect("utf-8 path")).expect("open database");
    let stats = load_streaming(&loopback_opts(port, SHAPE_QUERY), &db).expect("the pull succeeds");
    assert_eq!(stats.nodes_new, 2, "{image}: the two captured nodes");
    assert_eq!(stats.rels_new, 1, "{image}: the captured relationship");
    server.join().ok();
    (dir, db)
}

/// `(variable, property key, literal valueType spelling)` for every property
/// both captured `mappable` bodies carry on the `TypedSample` node (`n`) and
/// the `TYPED_REL` relationship (`r`). The spellings are the engine's
/// `valueType()` output (CIP-90), asserted literally.
const COMMON_TYPES: &[(&str, &str, &str)] = &[
    ("n", "boolean", "BOOLEAN NOT NULL"),
    ("n", "date", "DATE NOT NULL"),
    ("n", "datetime_named_zone", "ZONED DATETIME NOT NULL"),
    ("n", "datetime_offset", "ZONED DATETIME NOT NULL"),
    ("n", "duration", "DURATION NOT NULL"),
    ("n", "float", "FLOAT NOT NULL"),
    ("n", "integer", "INTEGER NOT NULL"),
    ("n", "list_boolean", "LIST<BOOLEAN NOT NULL> NOT NULL"),
    ("n", "list_date", "LIST<DATE NOT NULL> NOT NULL"),
    (
        "n",
        "list_datetime",
        "LIST<ZONED DATETIME NOT NULL> NOT NULL",
    ),
    ("n", "list_duration", "LIST<DURATION NOT NULL> NOT NULL"),
    ("n", "list_float", "LIST<FLOAT NOT NULL> NOT NULL"),
    ("n", "list_integer", "LIST<INTEGER NOT NULL> NOT NULL"),
    (
        "n",
        "list_localdatetime",
        "LIST<LOCAL DATETIME NOT NULL> NOT NULL",
    ),
    ("n", "list_localtime", "LIST<LOCAL TIME NOT NULL> NOT NULL"),
    ("n", "list_point", "LIST<POINT NOT NULL> NOT NULL"),
    ("n", "list_string", "LIST<STRING NOT NULL> NOT NULL"),
    ("n", "list_time", "LIST<ZONED TIME NOT NULL> NOT NULL"),
    ("n", "localdatetime", "LOCAL DATETIME NOT NULL"),
    ("n", "localtime", "LOCAL TIME NOT NULL"),
    ("n", "name", "STRING NOT NULL"),
    ("n", "point_cartesian", "POINT NOT NULL"),
    ("n", "point_cartesian_3d", "POINT NOT NULL"),
    ("n", "point_wgs84", "POINT NOT NULL"),
    ("n", "point_wgs84_3d", "POINT NOT NULL"),
    ("n", "string", "STRING NOT NULL"),
    ("n", "time", "ZONED TIME NOT NULL"),
    ("r", "at", "POINT NOT NULL"),
    ("r", "since", "DATE NOT NULL"),
];

/// The three properties only the 2026.09 body carries: a native UUID, a
/// `LIST<UUID>`, and a `VECTOR` (the 5.26 kernel has no `uuid()`/`vector()`
/// functions, so its captured node lacks them).
const TYPES_2026_09: &[(&str, &str, &str)] = &[
    ("n", "uuid", "UUID NOT NULL"),
    ("n", "list_uuid", "LIST<UUID NOT NULL> NOT NULL"),
    ("n", "vector", "VECTOR<FLOAT NOT NULL>(2) NOT NULL"),
];

/// Every temporal and point property — list elements included — compared
/// with the matching Cypher constructor applied to the fixture `_value`; the
/// scalar and scalar-list properties are pinned too, so no property silently
/// changes value.
const COMMON_VALUES: &[&str] = &[
    "n.name = 'mappable'",
    "n.integer = 42",
    "n.float = 3.14159",
    "n.string = 'hello'",
    "n.boolean = true",
    "n.date = date('2015-03-26')",
    "n.time = time('12:50:35.556+01:00')",
    "n.localtime = localtime('12:50:35.556')",
    "n.localdatetime = localdatetime('2015-07-04T19:32:24')",
    "n.datetime_offset = datetime('2015-11-21T21:40:32.142-01:00')",
    "n.datetime_named_zone = datetime('2015-11-21T21:40:32.142Z[Antarctica/Troll]')",
    "n.duration = duration('P14DT16H12M')",
    "n.point_cartesian = point({srid: 7203, x: 1.2, y: 3.4})",
    "n.point_cartesian_3d = point({srid: 9157, x: 1.0, y: 2.0, z: 3.0})",
    "n.point_wgs84 = point({srid: 4326, longitude: -0.1278, latitude: 51.5074})",
    "n.point_wgs84_3d = point({srid: 4979, longitude: -0.1278, latitude: 51.5074, height: 11.0})",
    "r.since = date('2020-01-01')",
    "r.at = point({srid: 4326, longitude: 10.0, latitude: 20.0})",
    "n.list_integer = [1, 2, 3]",
    "n.list_float = [1.5, 2.5]",
    "n.list_string = ['a', 'b']",
    "n.list_boolean = [true, false]",
    "n.list_date[0] = date('2020-01-01')",
    "n.list_date[1] = date('2020-01-02')",
    "n.list_time[0] = time('01:02:03Z')",
    "n.list_time[1] = time('04:05:06+02:00')",
    "n.list_localtime[0] = localtime('01:02:03')",
    "n.list_localtime[1] = localtime('04:05:06')",
    "n.list_localdatetime[0] = localdatetime('2020-01-01T10:00:00')",
    "n.list_localdatetime[1] = localdatetime('2020-06-01T10:00:00')",
    "n.list_datetime[0] = datetime('2020-01-01T10:00:00+01:00[Europe/Stockholm]')",
    "n.list_datetime[1] = datetime('2020-06-01T10:00:00+02:00')",
    "n.list_duration[0] = duration('P1D')",
    "n.list_duration[1] = duration('PT2H30M')",
    "n.list_point[0] = point({srid: 4326, longitude: 0.0, latitude: 0.0})",
    "n.list_point[1] = point({srid: 4326, longitude: 1.0, latitude: 2.0})",
];

/// The 2026.09-only values: the native UUID, the `LIST<UUID>`, and the
/// `VECTOR` compare against their constructors.
const VALUES_2026_09: &[&str] = &[
    "n.uuid = uuid('550e8400-e29b-41d4-a716-446655440000')",
    "n.vector = vector([1.0, 2.0], 2, FLOAT64)",
    "n.list_uuid[0] = uuid('550e8400-e29b-41d4-a716-446655440000')",
    "n.list_uuid[1] = uuid('00000000-0000-0000-0000-000000000000')",
];

/// One scalar `Value` from a query that must return exactly one row.
fn one_value(db: &Database, query: &str) -> Value {
    let mut conn = db.connect().expect("connect");
    let result = conn
        .query(query)
        .unwrap_or_else(|error| panic!("{query}: {error}"));
    assert_eq!(result.rows.len(), 1, "{query}");
    result.rows[0][0].clone()
}

/// `MATCH (n) RETURN count(n)` on the destination.
fn count_nodes(db: &Database) -> i64 {
    match one_value(db, "MATCH (n) RETURN count(n)") {
        Value::Int(n) => n,
        other => panic!("count(n) returned {other:?}"),
    }
}

/// Assert every `(variable, key, spelling)` row of the table: one query
/// returns `valueType()` for all of them, and each column equals its literal
/// spelling.
fn assert_value_types(db: &Database, table: &[(&str, &str, &str)]) {
    let columns = table
        .iter()
        .map(|(var, key, _)| format!("valueType({var}.{key})"))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!("MATCH (n:TypedSample)-[r:TYPED_REL]->() RETURN {columns}");
    let mut conn = db.connect().expect("connect");
    let result = conn
        .query(&query)
        .unwrap_or_else(|error| panic!("{query}: {error}"));
    assert_eq!(result.rows.len(), 1, "one TypedSample: {query}");
    for (index, (var, key, spelling)) in table.iter().enumerate() {
        assert_eq!(
            result.rows[0][index],
            Value::String((*spelling).into()),
            "valueType({var}.{key})"
        );
    }
}

/// Assert one boolean expression returns true on the pulled graph.
fn assert_true(db: &Database, expression: &str) {
    let query = format!("MATCH (n:TypedSample)-[r:TYPED_REL]->() RETURN {expression}");
    assert_eq!(one_value(db, &query), Value::Bool(true), "{expression}");
}

/// Whether the typed cell carries `label`.
fn has_label(cell: &Json, label: &str) -> bool {
    cell["_value"]["_labels"]
        .as_array()
        .is_some_and(|labels| labels.iter().any(|entry| entry == label))
}

/// Assert the table's `(variable, key)` pairs are exactly the captured
/// body's `TypedSample` node and `TYPED_REL` relationship properties: no
/// property missing from the table, none invented.
fn assert_table_covers_fixture(image: &str, table: &[(&str, &str, &str)]) {
    let served = body_json(image, "mappable");
    let mut fixture_keys = BTreeSet::new();
    for row in served["data"]["values"].as_array().expect("rows") {
        for cell in row.as_array().expect("cells") {
            let var = match cell["$type"].as_str() {
                Some("Node") if has_label(cell, "TypedSample") => "n",
                Some("Relationship") if cell["_value"]["_type"] == "TYPED_REL" => "r",
                _ => continue,
            };
            for key in cell["_value"]["_properties"]
                .as_object()
                .expect("properties")
                .keys()
            {
                fixture_keys.insert((var.to_string(), key.clone()));
            }
        }
    }
    let table_keys: BTreeSet<(String, String)> = table
        .iter()
        .map(|(var, key, _)| ((*var).to_string(), (*key).to_string()))
        .collect();
    assert_eq!(
        fixture_keys, table_keys,
        "{image}: the type table covers every fixture property"
    );
}

/// The whole type-fidelity contract on an open `db`: the flattened type
/// tables cover exactly the fixture's properties and `valueType` spells each
/// back, and every value check holds.
fn assert_type_fidelity(
    db: &Database,
    image: &str,
    tables: &[&[(&str, &str, &str)]],
    values: &[&[&str]],
) {
    let table: Vec<(&str, &str, &str)> = tables.iter().flat_map(|t| t.iter().copied()).collect();
    assert_table_covers_fixture(image, &table);
    assert_value_types(db, &table);
    for expressions in values {
        for expression in expressions.iter() {
            assert_true(db, expression);
        }
    }
}

/// The whole round-trip contract for one `image`: pull its captured
/// `mappable` body, assert type fidelity on the open store, then drop and
/// reopen the store and assert it again, so the types are shown to survive
/// the pull's checkpoint. `image` flows into every assertion so failures
/// name the image they came from.
fn assert_roundtrip_and_reopen(image: &str, tables: &[&[(&str, &str, &str)]], values: &[&[&str]]) {
    let (dir, db) = pull_mappable(image);
    assert_type_fidelity(&db, image, tables, values);
    drop(db);
    let db = Database::open(dir.path().to_str().expect("utf-8 path")).expect("reopen");
    assert_type_fidelity(&db, image, tables, values);
}

/// Every property of the 2026.09 `mappable` body round-trips with its exact
/// engine type and value, and the types survive the pull's checkpoint
/// (drop + reopen).
#[test]
fn mappable_2026_09_roundtrips_every_property_type() {
    assert_roundtrip_and_reopen(
        IMAGE_2026_09,
        &[COMMON_TYPES, TYPES_2026_09],
        &[COMMON_VALUES, VALUES_2026_09],
    );
}

/// The same round-trip for the 5.26 body, whose node has neither UUID nor
/// VECTOR properties (ADR-0181: those value types do not exist there).
#[test]
fn mappable_5_26_roundtrips_every_property_type() {
    assert_roundtrip_and_reopen(IMAGE_5_26, &[COMMON_TYPES], &[COMMON_VALUES]);
}

/// The pulled values are real engine values, not lookalikes: a pulled
/// WGS-84 point is distance 0 from its literal, a pulled DATE plus one day
/// is the next date, and a pulled ZONED DATETIME orders before itself plus
/// one second.
#[test]
fn pulled_points_and_temporals_do_arithmetic() {
    for image in [IMAGE_2026_09, IMAGE_5_26] {
        let (_dir, db) = pull_mappable(image);
        assert_eq!(
            one_value(
                &db,
                "MATCH (n:TypedSample) RETURN point.distance(n.point_wgs84, point({srid: 4326, longitude: -0.1278, latitude: 51.5074}))"
            ),
            Value::Float(0.0),
            "{image}: point.distance"
        );
        assert_eq!(
            one_value(
                &db,
                "MATCH (n:TypedSample) RETURN n.date + duration('P1D') = date('2015-03-27')"
            ),
            Value::Bool(true),
            "{image}: date + duration"
        );
        assert_eq!(
            one_value(
                &db,
                "MATCH (n:TypedSample) RETURN n.datetime_named_zone < n.datetime_named_zone + duration('PT1S')"
            ),
            Value::Bool(true),
            "{image}: zoned datetime ordering"
        );
    }
}

/// The `unmappable` body's `blob` is a `Base64` BYTE ARRAY the engine cannot
/// represent: the pull fails with `LoadError::TypedWire` naming the element
/// id, the property and tag, and nothing is materialized — not in the open
/// store, and not in the checkpoint after a reopen (ADR-0181 item 7). The
/// reason is the publicly nameable `DecodeError`: this `tests/` crate is a
/// crate of its own, so constructing and matching the variant here exercises
/// the public re-export, not a crate-internal path.
#[test]
fn unmappable_body_fails_the_pull_and_leaves_no_node() {
    for image in [IMAGE_2026_09, IMAGE_5_26] {
        let element = unmappable_element_id(image);
        let (server, port) = spawn_single_response("200 OK", body(image, "unmappable"));
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_str().expect("utf-8 path").to_string();
        let db = Database::open(&path).expect("open database");
        let err = load_streaming(
            &loopback_opts(port, "MATCH (u:TypedUnmappable) RETURN u"),
            &db,
        )
        .expect_err("the Base64 property fails the pull");
        let message = err.to_string();
        let wire = match err {
            LoadError::TypedWire(wire) => wire,
            other => panic!("{image}: expected LoadError::TypedWire, got {other:?}"),
        };
        assert_eq!(wire.element, element, "{image}");
        assert_eq!(wire.property, "blob", "{image}");
        assert_eq!(
            wire.error,
            DecodeError::Unsupported {
                tag: "Base64".into()
            },
            "{image}"
        );
        assert!(message.contains(&element), "{image}: {message}");
        assert!(message.contains("\"blob\""), "{image}: {message}");
        assert!(message.contains("Base64"), "{image}: {message}");
        assert_eq!(count_nodes(&db), 0, "{image}: nothing materialized");
        drop(db);
        let db = Database::open(&path).expect("reopen");
        assert_eq!(
            count_nodes(&db),
            0,
            "{image}: nothing materialized after reopen"
        );
        drop(db);
        server.join().ok();
    }
}
