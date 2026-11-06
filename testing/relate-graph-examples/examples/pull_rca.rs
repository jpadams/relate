//! Root-cause diagnostic for the pull materialization scaling wall (LABS-155).
//!
//! Three sections:
//!
//! 1. Per-statement shapes: times the statement shapes `relate-load`
//!    issued per element (`CREATE (n:L) SET n = $props RETURN n` and
//!    `MATCH (a), (b) WHERE id(a) = $s AND id(b) = $e CREATE (a)-[r:T]->(b) ...`)
//!    plus batched `UNWIND` variants at increasing graph sizes, to separate
//!    per-query overhead from super-linear work inside a single statement.
//! 2. Real loader: generates a synthetic Query API v2 response for the two
//!    shape statements of `pull::pull_whole_database` (nodes, then rels
//!    carrying their endpoints), serves it over loopback HTTP through the
//!    `relate_test_support::loopback` fixture, and times
//!    `relate_load::load_streaming` (checkpoint included) into a fresh
//!    on-disk database at 60,000 nodes + 40,000 rels and 330,000 nodes +
//!    100,000 rels. Each size prints one `loader ...` line and panics unless
//!    every element loaded.
//! 3. List-carry (LABS-156): times carrying a collected list of N nodes
//!    through 2000 UNWIND rows (`list-carry N=<N> <label> <secs>s`), the
//!    shape that goes quadratic when each variable read deep-clones the list.
//!
//! Run: cargo run --release -p relate-graph-examples --example pull_rca

use std::collections::BTreeMap;
use std::time::Instant;

use relate::{Connection, Database, Value};
use relate_graph_examples::pull::WHOLE_DATABASE_QUERY;
use relate_load::load_streaming;
use relate_test_support::loopback::{loopback_opts, spawn_scripted_responses};
use serde_json::{json, Value as Json};

fn create_nodes(conn: &mut Connection, n: usize) {
    let mut params = BTreeMap::new();
    params.insert("n".into(), Value::Int(n as i64));
    conn.query_with_params("UNWIND range(1, $n) AS i CREATE (:Seed {i: i})", params)
        .expect("seed");
}

/// Isolate the write path: bare create, label-only, props-only, and a pure
/// read, so a linear-in-N cost can be attributed to labels, properties, or
/// per-statement setup.
fn time_variant(conn: &mut Connection, count: usize, statement: &str) -> f64 {
    let start = Instant::now();
    for i in 0..count {
        let mut params = BTreeMap::new();
        params.insert("props".into(), Value::Int(i as i64));
        conn.query_with_params(statement, params).expect("variant");
    }
    start.elapsed().as_secs_f64()
}

/// Time one statement that batches `rows` of work through
/// `UNWIND range(1, $props)`. Returns the elapsed seconds and the writes the
/// statement actually did (nodes created + properties set), so an empty
/// UNWIND cannot hide behind the "rows per statement" label.
fn time_batched(conn: &mut Connection, rows: usize, statement: &str) -> (f64, i64) {
    let mut params = BTreeMap::new();
    params.insert("props".into(), Value::Int(rows as i64));
    let start = Instant::now();
    let result = conn.query_with_params(statement, params).expect("batched");
    let writes = result.stats.nodes_created + result.stats.properties_set;
    (start.elapsed().as_secs_f64(), writes)
}

fn time_rel_creates(conn: &mut Connection, count: usize) -> f64 {
    let start = Instant::now();
    for i in 0..count {
        let mut params = BTreeMap::new();
        params.insert("s".into(), Value::Int(1 + (i as i64 % 100)));
        params.insert("e".into(), Value::Int(1 + ((i as i64 + 1) % 100)));
        conn.query_with_params(
            "MATCH (a), (b) WHERE id(a) = $s AND id(b) = $e CREATE (a)-[:R]->(b) RETURN a",
            params,
        )
        .expect("rel create");
    }
    start.elapsed().as_secs_f64()
}

const LIST_CARRY_REPRO: &str = "UNWIND range(1,$n) AS i CREATE (n:N {i: i}) \
     WITH collect(n) AS ns UNWIND range(0,1999) AS k \
     WITH k, ns[k] AS a, ns[(k+1) % $n] AS b RETURN count(*)";
const LIST_CARRY_CARRY_ONLY: &str = "UNWIND range(1,$n) AS i CREATE (n:N {i: i}) \
     WITH collect(n) AS ns UNWIND range(0,1999) AS k RETURN count(*)";
const LIST_CARRY_CREATE_COLLECT: &str = "UNWIND range(1,$n) AS i CREATE (n:N {i: i}) \
     WITH collect(n) AS ns RETURN size(ns)";

/// LABS-156: carry a collected list of N nodes through 2000 UNWIND rows.
/// `repro` reads two list elements per row, `carry-only` just carries the
/// list, and `create+collect` is the shared setup cost. Each statement runs
/// on its own fresh database so the three stay independent.
fn list_carry() {
    for n in [2_000usize, 20_000] {
        for (label, statement, expected) in [
            ("repro", LIST_CARRY_REPRO, 2_000),
            ("carry-only", LIST_CARRY_CARRY_ONLY, 2_000),
            ("create+collect", LIST_CARRY_CREATE_COLLECT, n as i64),
        ] {
            let db = Database::open(":memory:").expect("db");
            let mut conn = db.connect().expect("conn");
            let mut params = BTreeMap::new();
            params.insert("n".into(), Value::Int(n as i64));
            let start = Instant::now();
            let result = conn
                .query_with_params(statement, params)
                .expect("list-carry");
            let secs = start.elapsed().as_secs_f64();
            assert_eq!(
                result.rows[0][0],
                Value::Int(expected),
                "list-carry N={n} {label}"
            );
            println!("list-carry N={n} {label:<15} {secs:8.3}s");
        }
    }
}

fn main() {
    per_statement_shapes();
    for (nodes, rels) in [(60_000, 40_000), (330_000, 100_000)] {
        bench_loader(nodes, rels);
    }
    list_carry();
}

fn per_statement_shapes() {
    for n in [1_000usize, 5_000, 20_000] {
        let db = Database::open(":memory:").expect("db");
        let mut conn = db.connect().expect("conn");
        create_nodes(&mut conn, n);
        let node_secs = time_variant(
            &mut conn,
            200,
            "CREATE (n:Extra) SET n = {v: $props} RETURN n",
        );
        let rel_secs = time_rel_creates(&mut conn, 200);
        println!(
            "graph nodes {n:>6}: 200 node-creates {node_secs:7.3}s ({:6.1} ms/op) | \
             200 rel-creates {rel_secs:7.3}s ({:6.1} ms/op)",
            node_secs * 1000.0 / 200.0,
            rel_secs * 1000.0 / 200.0,
        );
        for (label, statement) in [
            ("read-only (RETURN 1)", "RETURN 1"),
            ("bare create", "CREATE (n) RETURN n"),
            ("label-only create", "CREATE (n:Extra2) RETURN n"),
            (
                "props-only create",
                "CREATE (n) SET n = {v: $props} RETURN n",
            ),
            (
                "existing-node SET (no create)",
                "MATCH (n:Seed) WHERE id(n) = $props SET n.touched = 1 RETURN n",
            ),
        ] {
            let secs = time_variant(&mut conn, 200, statement);
            println!(
                "    {label:<32} {:7.3}s ({:6.1} ms/op)",
                secs,
                secs * 1000.0 / 200.0
            );
        }
        // Does batching with UNWIND remove the wall, or does the label path
        // stay O(N) even in one statement? 200 rows per statement.
        const BATCH_ROWS: usize = 200;
        for (label, statement) in [
            (
                "batched labeled create",
                "UNWIND range(1, $props) AS i CREATE (n:Batch {v: i})",
            ),
            (
                "batched labeled match+SET",
                "UNWIND range(1, $props) AS i MATCH (n:Seed) WHERE id(n) = i SET n.x = 1",
            ),
        ] {
            let (secs, writes) = time_batched(&mut conn, BATCH_ROWS, statement);
            println!(
                "    {label:<32} {:7.3}s ({:6.1} ms/row, \
                 {BATCH_ROWS} rows/statement, {writes} writes)",
                secs,
                secs * 1000.0 / BATCH_ROWS as f64,
            );
        }
    }
}

const LABELS: [&str; 6] = ["Person", "Company", "Account", "Device", "Address", "Event"];
const REL_TYPES: [&str; 4] = ["KNOWS", "OWNS", "USES", "LOCATED_AT"];

/// splitmix64: a deterministic synthetic graph without a rand dependency.
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Node `i` as a typed Query API v2 node value (ADR-0181): 1 to 3 distinct
/// labels, 2 properties. Always the same value for the same `i`.
fn node(i: usize) -> Json {
    let h = mix(i as u64);
    let first = (h >> 8) as usize;
    let labels: Vec<&str> = (0..1 + h as usize % 3)
        .map(|k| LABELS[(first + k) % LABELS.len()])
        .collect();
    json!({
        "$type": "Node",
        "_value": {
            "_element_id": format!("4:bench:{i}"),
            "_labels": labels,
            "_properties": {
                "name": {"$type": "String", "_value": format!("node-{i}")},
                "rank": {"$type": "Integer", "_value": (h % 1000).to_string()},
            },
        },
    })
}

/// Rel `j` as a typed `x, e, y` row: its start node, the rel value, its end
/// node, with endpoints drawn from the first `nodes` nodes.
fn rel_row(j: usize, nodes: usize) -> Json {
    let h = mix(!(j as u64));
    let (start, end) = (h as usize % nodes, (h >> 32) as usize % nodes);
    let rel = json!({
        "$type": "Relationship",
        "_value": {
            "_element_id": format!("5:bench:{j}"),
            "_start_node_element_id": format!("4:bench:{start}"),
            "_end_node_element_id": format!("4:bench:{end}"),
            "_type": REL_TYPES[(h >> 16) as usize % REL_TYPES.len()],
            "_properties": {
                "weight": {"$type": "Integer", "_value": (h % 100).to_string()},
            },
        },
    });
    json!([node(start), rel, node(end)])
}

/// A Query API v2 success body: `fields`, then `rows` as `data.values`.
fn envelope(fields: &[&str], rows: impl Iterator<Item = Json>) -> String {
    let values: Vec<Json> = rows.collect();
    json!({"data": {"fields": fields, "values": values}, "errors": []}).to_string()
}

/// The two Query API v2 bodies of `pull::pull_whole_database`: every node
/// as an `e` row, then every rel as an `x, e, y` row carrying its endpoint
/// nodes (identical to their node rows, so they dedupe).
fn synthetic_bodies(nodes: usize, rels: usize) -> Vec<String> {
    vec![
        envelope(&["e"], (0..nodes).map(|i| json!([node(i)]))),
        envelope(&["x", "e", "y"], (0..rels).map(|j| rel_row(j, nodes))),
    ]
}

/// Time `load_streaming` (checkpoint included) on a synthetic
/// whole-database pull into a fresh on-disk database; panics unless every
/// element loaded.
fn bench_loader(nodes: usize, rels: usize) {
    let (server, port) = spawn_scripted_responses("200 OK", synthetic_bodies(nodes, rels));
    let dir = tempfile::tempdir().expect("temp dir");
    let db = Database::open(dir.path().to_str().expect("UTF-8 temp path")).expect("open db");
    let opts = loopback_opts(port, WHOLE_DATABASE_QUERY);
    let start = Instant::now();
    let stats = load_streaming(&opts, &db).expect("load_streaming");
    let secs = start.elapsed().as_secs_f64();
    println!(
        "loader {nodes} nodes {rels} rels: {secs:.3} s ({:.1} us/element) nodes_new={} rels_new={}",
        secs * 1e6 / (nodes + rels) as f64,
        stats.nodes_new,
        stats.rels_new,
    );
    server.join().expect("loopback server");
    assert_eq!(stats.nodes_new, nodes, "nodes_new");
    assert_eq!(stats.rels_new, rels, "rels_new");
    let mut conn = db.connect().expect("connect");
    let count = conn
        .query("MATCH (n) RETURN count(n)")
        .expect("count nodes");
    assert_eq!(
        count.rows[0][0].as_int(),
        Some(nodes as i64),
        "MATCH (n) RETURN count(n)"
    );
}
