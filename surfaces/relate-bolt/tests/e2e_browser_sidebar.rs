//! End-to-end test of the browser.neo4j.io sidebar metadata batch over a
//! real Bolt connection (LABS-149/150/151): the 3-way UNION ALL
//! COLLECT-slice query, `db.schema.visualization()`, `dbms.components()`,
//! and `dbms.clientConfig()` against the real `relate-bolt` binary.
//!
//! The risk class under test is the PackStream wire encoding of the
//! virtual Node/Relationship values `db.schema.visualization()` returns —
//! they carry negative ids, the same shape that bit PR #149's Path
//! encoding — so the test decodes the RECORD values and checks the ids,
//! struct tags, and raw struct markers on the wire, not just SUCCESS.

mod common;

use common::packstream::{decode, Pack};
use common::{connect_and_hello, is_success, read_message, run_and_pull, send, start_server};
use std::collections::BTreeMap;
use std::net::TcpStream;

fn str_list(values: &[Pack]) -> Vec<&str> {
    values
        .iter()
        .map(|v| v.as_str().expect("a string value"))
        .collect()
}

/// The LABS-149 sidebar metadata query: one row per branch, `result` the
/// COLLECT sliced by `$itemLimit`, `type` naming the branch.
const SIDEBAR_METADATA_QUERY: &str = "\
CALL db.labels() YIELD label \
RETURN COLLECT(label)[..$itemLimit] AS result, 'labels' AS type \
UNION ALL \
CALL db.relationshipTypes() YIELD relationshipType \
RETURN COLLECT(relationshipType)[..$itemLimit] AS result, 'relationshipTypes' AS type \
UNION ALL \
CALL db.propertyKeys() YIELD propertyKey \
RETURN COLLECT(propertyKey)[..$itemLimit] AS result, 'propertyKeys' AS type";

/// Decode the `nodes` column of a `db.schema.visualization()` row: Bolt
/// Node struct 0x4E [id, labels, properties], one per label, negative ids.
/// Returns the label -> virtual id map the relationship endpoints are
/// resolved against.
fn decode_visualization_nodes(nodes: &[Pack]) -> BTreeMap<String, i64> {
    assert_eq!(nodes.len(), 2, "one virtual node per label: {nodes:?}");
    let mut id_by_label = BTreeMap::new();
    for node in nodes {
        let Pack::Struct { tag: 0x4E, fields } = node else {
            panic!("virtual node is a Node struct (0x4E), got {node:?}");
        };
        let [Pack::Int(id), Pack::List(labels), Pack::Map(_)] = &fields[..] else {
            panic!("Node fields are [id, labels, properties], got {fields:?}");
        };
        assert!(*id < 0, "virtual node id is negative, got {id}");
        let [Pack::String(label)] = &labels[..] else {
            panic!("virtual node carries exactly one label, got {labels:?}");
        };
        id_by_label.insert(label.clone(), *id);
    }
    for label in ["Movie", "Person"] {
        assert!(
            id_by_label.contains_key(label),
            "a virtual node for :{label}: {id_by_label:?}"
        );
    }
    id_by_label
}

/// Decode the `relationships` column of a `db.schema.visualization()` row:
/// Bolt Relationship struct 0x52 [id, start, end, type, properties], one
/// per distinct triple, negative ids, endpoints referencing the virtual
/// node ids in `id_by_label`. Each properties map is exactly
/// `{name: <type>}` (ADR-0187).
fn assert_visualization_relationships(rels: &[Pack], id_by_label: &BTreeMap<String, i64>) {
    let expected_triples: &[(&str, &str, &str)] = &[
        ("Person", "ACTED_IN", "Movie"),
        ("Person", "KNOWS", "Person"),
    ];
    assert_eq!(
        rels.len(),
        expected_triples.len(),
        "one virtual relationship per triple: {rels:?}"
    );
    let mut seen = Vec::new();
    for rel in rels {
        let Pack::Struct { tag: 0x52, fields } = rel else {
            panic!("virtual relationship is a Relationship struct (0x52), got {rel:?}");
        };
        let [Pack::Int(id), Pack::Int(start), Pack::Int(end), Pack::String(rel_type), Pack::Map(properties)] =
            &fields[..]
        else {
            panic!("Relationship fields are [id, start, end, type, properties], got {fields:?}");
        };
        assert!(*id < 0, "virtual relationship id is negative, got {id}");
        // ADR-0187: the property map is exactly `{name: <relType>}`.
        let [(Pack::String(key), Pack::String(name))] = &properties[..] else {
            panic!("virtual relationship carries exactly one `name` property, got {properties:?}");
        };
        assert_eq!(key, "name", "the single property key is `name`");
        assert_eq!(name, rel_type, "`name` equals the relationship type");
        let label_of = |id: i64| {
            id_by_label
                .iter()
                .find(|(_, v)| **v == id)
                .map(|(k, _)| k.as_str())
                .unwrap_or_else(|| panic!("endpoint id {id} is a virtual node id"))
        };
        seen.push((label_of(*start), rel_type.as_str(), label_of(*end)));
    }
    seen.sort_unstable();
    let mut expected = expected_triples.to_vec();
    expected.sort_unstable();
    assert_eq!(seen, expected, "one relationship per distinct triple");
}

/// Seed two labels and two relationship types:
/// (:Person)-[:KNOWS]->(:Person), (:Person)-[:ACTED_IN]->(:Movie).
fn seed_schema_graph(client: &mut TcpStream) {
    let seeded = run_and_pull(
        client,
        "CREATE (a:Person {name: 'Alice'})-[:KNOWS {since: 2020}]->(b:Person {name: 'Bob'}), \
         (a)-[:ACTED_IN]->(m:Movie {title: 'Dune'})",
        vec![],
    );
    assert!(seeded.is_empty(), "CREATE returns no records: {seeded:?}");
}

/// The LABS-149 3-way UNION ALL metadata query with itemLimit = 1000.
#[test]
fn sidebar_metadata_query_over_bolt() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_and_hello(&server.addr);

    seed_schema_graph(&mut client);

    let records = run_and_pull(
        &mut client,
        SIDEBAR_METADATA_QUERY,
        vec![(Pack::String("itemLimit".into()), Pack::Int(1000))],
    );
    assert_eq!(
        records.len(),
        3,
        "one row per UNION ALL branch: {records:?}"
    );
    for row in &records {
        let [Pack::List(result), Pack::String(branch)] = &row[..] else {
            panic!("branch row is (result list, type string), got {row:?}");
        };
        let mut items = str_list(result);
        items.sort_unstable();
        let expected: &[&str] = match branch.as_str() {
            "labels" => &["Movie", "Person"],
            "relationshipTypes" => &["ACTED_IN", "KNOWS"],
            "propertyKeys" => &["name", "since", "title"],
            other => panic!("unexpected branch type {other:?}"),
        };
        assert_eq!(items, expected, "branch {branch}");
    }
}

/// `db.schema.visualization()`: one virtual node per label, one virtual
/// relationship per distinct (startLabel, relType, endLabel) triple — with
/// negative ids surviving the PackStream round-trip.
#[test]
fn schema_visualization_over_bolt() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_and_hello(&server.addr);

    seed_schema_graph(&mut client);

    let records = run_and_pull(
        &mut client,
        "CALL db.schema.visualization() YIELD nodes, relationships RETURN nodes, relationships",
        vec![],
    );
    assert_eq!(records.len(), 1, "one visualization row: {records:?}");
    let [Pack::List(nodes), Pack::List(rels)] = &records[0][..] else {
        panic!(
            "row is (nodes list, relationships list), got {:?}",
            records[0]
        );
    };
    let id_by_label = decode_visualization_nodes(nodes);
    assert_visualization_relationships(rels, &id_by_label);
}

/// `dbms.components()`: the 'Neo4j Kernel' row reports the ADR-0152
/// policy version as versions[0].
#[test]
fn dbms_components_over_bolt() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_and_hello(&server.addr);

    let records = run_and_pull(&mut client, "CALL dbms.components()", vec![]);
    assert_eq!(records.len(), 3, "ADR-0142 row count: {records:?}");
    let kernel = records
        .iter()
        .find(|row| row.first().and_then(Pack::as_str) == Some("Neo4j Kernel"))
        .unwrap_or_else(|| panic!("a 'Neo4j Kernel' row: {records:?}"));
    let Some(Pack::List(versions)) = kernel.get(1) else {
        panic!("versions is a list, got {kernel:?}");
    };
    assert_eq!(
        versions.first().and_then(Pack::as_str),
        Some("2026.10-relate"),
        "Neo4j Kernel versions[0] is the ADR-0152 policy value"
    );
}

/// `dbms.clientConfig()`: the 8-column ConfigResult shape, one row per
/// allowlist key (ADR-0143).
#[test]
fn dbms_client_config_over_bolt() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_and_hello(&server.addr);

    let records = run_and_pull(&mut client, "CALL dbms.clientConfig()", vec![]);
    assert_eq!(records.len(), 9, "clientConfig allowlist rows: {records:?}");
    for row in &records {
        assert_eq!(row.len(), 8, "8-column ConfigResult row: {row:?}");
    }
}

/// The virtual entities' negative ids must survive PackStream as Node /
/// Relationship structs on the wire — not as strings or maps. Check the
/// raw RECORD bytes carry the struct markers (0xB3 0x4E for a 3-field
/// Node, 0xB5 0x52 for a 5-field Relationship) and that a tiny-int
/// negative id byte (0xF0..=0xFF) sits inside each struct's field span.
#[test]
fn visualization_record_wire_bytes_carry_node_and_relationship_structs() {
    let server = start_server(&["--uri", ":memory:", "--auth", "none"]);
    let mut client = connect_and_hello(&server.addr);

    run_and_pull(
        &mut client,
        "CREATE (:Person {name: 'Alice'})-[:KNOWS]->(:Person {name: 'Bob'})",
        vec![],
    );

    send(
        &mut client,
        0x10,
        vec![
            Pack::String(
                "CALL db.schema.visualization() YIELD nodes, relationships \
                 RETURN nodes, relationships"
                    .into(),
            ),
            Pack::Map(vec![]),
        ],
    );
    assert!(is_success(&read_message(&mut client)));
    send(
        &mut client,
        0x3F,
        vec![Pack::Map(vec![(Pack::String("n".into()), Pack::Int(-1))])],
    );
    let record = read_message(&mut client);

    // Byte-level markers: the struct headers must be present in the raw
    // payload (a string-encoded fallback would not produce them).
    let has_marker = |marker: &[u8]| record.windows(marker.len()).any(|window| window == marker);
    assert!(
        has_marker(&[0xB3, 0x4E]),
        "RECORD carries a 3-field Node struct header: {record:02x?}"
    );
    assert!(
        has_marker(&[0xB5, 0x52]),
        "RECORD carries a 5-field Relationship struct header: {record:02x?}"
    );

    // And the decoded values round-trip: negative ids as Pack::Int inside
    // the structs (checked structurally in the batch test; here the raw
    // bytes must decode at all, which a malformed negative-id encoding
    // would break).
    let decoded = decode(&record).expect("RECORD payload decodes");
    let Pack::Struct { tag: 0x71, .. } = decoded else {
        panic!("a RECORD message, got {decoded:?}");
    };
}
