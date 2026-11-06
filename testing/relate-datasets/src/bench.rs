//! Benchmark harness: baseline topology logical bytes and expansion
//! throughput per dataset.
//!
//! Two measurements per dataset:
//!
//! - **Topology logical bytes** — the engine's own logical accounting
//!   (ADR-0042, `Database::memory_usage`) applied to a *topology twin*: an
//!   in-memory database that replays every node (labels, no properties) and
//!   every relationship (type and endpoints, no properties) of the dataset
//!   with no indexes. The twin is built with the same batched
//!   `UNWIND $rows … CREATE (n:$(row.labels))` / `elementId` seek pattern the
//!   pull loader uses (`relate-load`, ADR-0155), so rebuild cost stays
//!   linear. Total logical bytes are `memory_usage` on the dataset itself
//!   (live graph + indexes, all namespaces).
//! - **Expansion throughput** — a fixed traversal workload: one warm-up run,
//!   then `rounds` timed runs of (a) all-node 1-hop expansion
//!   (`MATCH (n)-[r]->(m) RETURN count(r)`, hops = count of traversed
//!   relationships) and (b) a 2-hop variable-length pass over a deterministic
//!   stride sample of seed nodes (`MATCH (n) WHERE id(n) % stride = 0
//!   MATCH (n)-[*2]->(m) RETURN count(m)`, hops = 2 × counted paths), with
//!   `stride = ceil(nodes / 4096)` so every graph pays a comparable 2-hop
//!   bill (graphs up to 4,096 nodes expand from every node). The reported
//!   hops/second is the best round.
//!
//! Datasets are either existing stores opened **read-only** (ADR-0136) or
//! synthetic memory shapes built from [`crate::shapes`]. The shared
//! `--db`/`--shape` scaffolding lives in [`crate::harness`].

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::harness::{build_shape, hardware_note, StoreDataset};
use crate::shapes::GraphShape;

/// Rows per `UNWIND $rows` batch when rebuilding the topology twin; matches
/// the pull loader's batch (`relate-load` `BATCH_ROWS`).
const TWIN_BATCH_ROWS: usize = 1_000;

/// Seed count the 2-hop pass aims for: `stride = ceil(nodes / target)`.
const TWO_HOP_SEED_TARGET: u64 = 4_096;

/// Where a benchmarked dataset came from; serialized as the report's `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatasetKind {
    /// An existing store opened read-only (`--db` / `--graph-examples-dir`).
    Store,
    /// A synthetic memory shape built in-process (`--shape`).
    SyntheticShape,
}

#[derive(Debug, Clone)]
pub struct BenchConfig {
    /// Existing stores (`--db NAME=PATH` / `--graph-examples-dir`).
    pub stores: Vec<StoreDataset>,
    /// Synthetic memory shapes built in-process (`--shape`; the binaries
    /// declare the default set when the flag is absent).
    pub shapes: Vec<GraphShape>,
    /// Closed-form logical target for each synthetic shape
    /// (`--shape-logical-mb`).
    pub shape_logical_bytes: u64,
    /// Integer properties per synthetic node (`--attributes`).
    pub attributes: u32,
    /// Timed rounds per traversal; the best round is reported (`--rounds`).
    pub rounds: u32,
    /// Optional report file (`--out`); the table always prints to stdout.
    pub out: Option<PathBuf>,
    /// Command line echoed into the report for reproduction.
    pub repro_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetReport {
    pub dataset: String,
    /// `store` for an opened database, `synthetic-shape` for a built shape.
    pub kind: DatasetKind,
    pub nodes: u64,
    pub rels: u64,
    pub topology_logical_bytes: u64,
    pub total_logical_bytes: u64,
    pub hops_per_second_1hop: f64,
    pub hops_per_second_2hop: f64,
    /// Wall seconds for the whole dataset pass (load, twin, traversals).
    pub seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    pub hardware: String,
    pub repro_command: String,
    pub datasets: Vec<DatasetReport>,
}

/// Run every configured dataset and return the report.
pub fn run(cfg: &BenchConfig) -> Result<BenchReport, String> {
    if cfg.stores.is_empty() && cfg.shapes.is_empty() {
        return Err("nothing to benchmark: pass --db/--graph-examples-dir or --shape".into());
    }
    if cfg.rounds == 0 {
        return Err("--rounds must be at least 1".into());
    }
    let mut datasets = Vec::new();
    for store in &cfg.stores {
        let db = relate::Database::open_read_only(&store.path.display().to_string())
            .map_err(|e| format!("{}: open read-only: {e}", store.name))?;
        datasets.push(bench_database(
            &store.name,
            DatasetKind::Store,
            &db,
            cfg.rounds,
        )?);
    }
    for shape in &cfg.shapes {
        let name = format!("shape-{shape}");
        let db = build_shape(*shape, cfg.shape_logical_bytes, cfg.attributes)
            .map_err(|e| format!("{name}: {e}"))?;
        datasets.push(bench_database(
            &name,
            DatasetKind::SyntheticShape,
            &db,
            cfg.rounds,
        )?);
    }
    Ok(BenchReport {
        hardware: hardware_note(),
        repro_command: cfg.repro_command.clone(),
        datasets,
    })
}

/// Count a single unsigned integer column from the first row of a query.
fn count(conn: &mut relate::Connection, cypher: &str) -> Result<u64, String> {
    let r = conn.query(cypher).map_err(|e| format!("{cypher}: {e}"))?;
    match r.rows.first().and_then(|row| row.first()) {
        Some(relate::Value::Int(n)) if *n >= 0 => Ok(*n as u64),
        other => Err(format!(
            "{cypher}: expected one non-negative integer, got {other:?}"
        )),
    }
}

/// Best hops/second over `rounds` timed runs of `cypher`, after one warm-up.
/// `hops_of` turns the single returned integer (relationships or paths) into
/// the hop count.
fn throughput(
    conn: &mut relate::Connection,
    cypher: &str,
    rounds: u32,
    hops_of: impl Fn(u64) -> u64,
) -> Result<f64, String> {
    conn.query(cypher).map_err(|e| format!("{cypher}: {e}"))?;
    let mut best = 0.0f64;
    for _ in 0..rounds {
        let started = Instant::now();
        let units = count(conn, cypher)?;
        let secs = started.elapsed().as_secs_f64();
        if secs > 0.0 {
            best = best.max(hops_of(units) as f64 / secs);
        }
    }
    Ok(best)
}

/// Measure one loaded database: counts, topology twin bytes, total logical
/// bytes, and 1-hop/2-hop expansion throughput.
pub fn bench_database(
    name: &str,
    kind: DatasetKind,
    db: &relate::Database,
    rounds: u32,
) -> Result<DatasetReport, String> {
    let started = Instant::now();
    let mut conn = db.connect().map_err(|e| e.to_string())?;
    let nodes = count(&mut conn, "MATCH (n) RETURN count(n)")?;
    let rels = count(&mut conn, "MATCH ()-[r]->() RETURN count(r)")?;
    let (total_logical_bytes, _) = db.memory_usage().map_err(|e| e.to_string())?;
    let topology_logical_bytes = topology_twin_bytes(&mut conn)?;
    let hops_per_second_1hop = throughput(
        &mut conn,
        "MATCH (n)-[r]->(m) RETURN count(r)",
        rounds,
        |hops| hops,
    )?;
    let stride = nodes.div_ceil(TWO_HOP_SEED_TARGET).max(1);
    let hops_per_second_2hop = throughput(
        &mut conn,
        &format!(
            "MATCH (n) WHERE id(n) % {stride} = 0 \
             MATCH (n)-[*2]->(m) RETURN count(m)"
        ),
        rounds,
        |paths| 2 * paths,
    )?;
    Ok(DatasetReport {
        dataset: name.to_string(),
        kind,
        nodes,
        rels,
        topology_logical_bytes,
        total_logical_bytes,
        hops_per_second_1hop,
        hops_per_second_2hop,
        seconds: started.elapsed().as_secs_f64(),
    })
}

/// Logical bytes of the topology twin: same nodes (labels only), same
/// relationships (type and endpoints only), no properties, no indexes.
fn topology_twin_bytes(source: &mut relate::Connection) -> Result<u64, String> {
    let twin = relate::Database::open(":memory:").map_err(|e| e.to_string())?;
    let mut dst = twin.connect().map_err(|e| e.to_string())?;
    let id_map = twin_nodes(source, &mut dst)?;
    twin_rels(source, &mut dst, &id_map)?;
    let (bytes, _) = twin.memory_usage().map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Replay every node of `source` (labels, no properties) into `dst`,
/// returning the source-elementId → twin-elementId map.
fn twin_nodes(
    source: &mut relate::Connection,
    dst: &mut relate::Connection,
) -> Result<HashMap<String, String>, String> {
    let node_rows = source
        .query("MATCH (n) RETURN elementId(n) AS eid, labels(n) AS labels")
        .map_err(|e| format!("node scan: {e}"))?;
    let mut id_map: HashMap<String, String> = HashMap::with_capacity(node_rows.rows.len());
    for batch in node_rows.rows.chunks(TWIN_BATCH_ROWS) {
        let rows: Vec<relate::Value> = batch
            .iter()
            .map(|row| {
                let [relate::Value::String(eid), labels @ relate::Value::List(_)] = &row[..] else {
                    return Err(format!("node scan row has unexpected shape: {row:?}"));
                };
                Ok(map_value(&[
                    ("eid", str_value(eid)),
                    ("labels", labels.clone()),
                ]))
            })
            .collect::<Result<_, String>>()?;
        let r = run_rows(
            dst,
            "UNWIND $rows AS row CREATE (n:$(row.labels)) \
             RETURN row.eid AS reid, elementId(n) AS lid",
            rows,
        )?;
        if r.rows.len() != batch.len() {
            return Err(format!(
                "twin node batch created {} rows, expected {}",
                r.rows.len(),
                batch.len()
            ));
        }
        for row in &r.rows {
            let [relate::Value::String(reid), relate::Value::String(lid)] = &row[..] else {
                return Err(format!("twin node batch returned unexpected row: {row:?}"));
            };
            id_map.insert(reid.to_string(), lid.to_string());
        }
    }
    Ok(id_map)
}

/// Replay every relationship of `source` (type and endpoints, no properties)
/// into `dst` through `id_map`, reconciling created vs. scanned counts.
fn twin_rels(
    source: &mut relate::Connection,
    dst: &mut relate::Connection,
    id_map: &HashMap<String, String>,
) -> Result<(), String> {
    let rel_rows = source
        .query("MATCH (a)-[r]->(b) RETURN elementId(a) AS s, elementId(b) AS e, type(r) AS t")
        .map_err(|e| format!("relationship scan: {e}"))?;
    let scanned = rel_rows.rows.len();
    let mut created = 0usize;
    for batch in rel_rows.rows.chunks(TWIN_BATCH_ROWS) {
        let mut rows: Vec<relate::Value> = Vec::with_capacity(batch.len());
        for row in batch {
            let [relate::Value::String(s), relate::Value::String(e), relate::Value::String(t)] =
                &row[..]
            else {
                return Err(format!(
                    "relationship scan row has unexpected shape: {row:?}"
                ));
            };
            let (Some(ts), Some(te)) = (id_map.get(s.as_ref()), id_map.get(e.as_ref())) else {
                return Err(format!(
                    "relationship {s}->{e} has an endpoint outside the scan"
                ));
            };
            rows.push(map_value(&[
                ("s", str_value(ts)),
                ("e", str_value(te)),
                ("t", str_value(t)),
            ]));
        }
        let r = run_rows(
            dst,
            "UNWIND $rows AS row \
             MATCH (a) WHERE elementId(a) = row.s \
             MATCH (b) WHERE elementId(b) = row.e \
             CREATE (a)-[r:$(row.t)]->(b) RETURN count(r) AS made",
            rows,
        )?;
        created += count_rows(&r, "made")?;
    }
    if created != scanned {
        return Err(format!(
            "twin created {created} relationships, expected {scanned}"
        ));
    }
    Ok(())
}

fn str_value(s: &str) -> relate::Value {
    relate::Value::String(Arc::from(s))
}

fn map_value(pairs: &[(&str, relate::Value)]) -> relate::Value {
    relate::Value::Map(Arc::new(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    ))
}

fn run_rows(
    conn: &mut relate::Connection,
    cypher: &str,
    rows: Vec<relate::Value>,
) -> Result<relate::QueryResult, String> {
    let mut params = BTreeMap::new();
    params.insert("rows".to_string(), relate::Value::List(Arc::new(rows)));
    conn.query_with_params(cypher, params)
        .map_err(|e| format!("{cypher}: {e}"))
}

/// Sum the single integer column `column` over all rows of a result.
fn count_rows(r: &relate::QueryResult, column: &str) -> Result<usize, String> {
    let idx = r
        .header
        .columns
        .iter()
        .position(|c| c == column)
        .ok_or_else(|| format!("result has no {column} column"))?;
    let mut total = 0usize;
    for row in &r.rows {
        match &row[idx] {
            relate::Value::Int(n) if *n >= 0 => total += *n as usize,
            other => {
                return Err(format!(
                    "{column} row is not a non-negative integer: {other:?}"
                ))
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::{self, census, IndexCounts, ShapeScale};

    fn small_shape_db(shape: GraphShape) -> relate::Database {
        let db = relate::Database::open(":memory:").unwrap();
        let mut conn = db.connect().unwrap();
        for statement in shapes::statements(shape, ShapeScale::Small, IndexCounts::NONE, 1).unwrap()
        {
            conn.query(&statement).unwrap();
        }
        drop(conn);
        db
    }

    #[test]
    fn ring_shape_reports_census_and_positive_throughput() {
        let db = small_shape_db(GraphShape::Ring);
        let report = bench_database("shape-ring", DatasetKind::SyntheticShape, &db, 1).unwrap();
        let want = census(GraphShape::Ring, ShapeScale::Small, IndexCounts::NONE, 1).unwrap();
        assert_eq!(report.nodes, want.nodes);
        assert_eq!(report.rels, want.relationships);
        assert!(report.topology_logical_bytes > 0);
        assert!(report.total_logical_bytes >= report.topology_logical_bytes);
        assert!(report.hops_per_second_1hop > 0.0);
        assert!(report.hops_per_second_2hop > 0.0);
    }

    #[test]
    fn twin_drops_properties_and_keeps_topology() {
        // The twin keeps every node/rel/label but no properties, so its
        // logical bytes stay below the shape's (each node carries `id`).
        let db = small_shape_db(GraphShape::Fanout);
        let report = bench_database("shape-fanout", DatasetKind::SyntheticShape, &db, 1).unwrap();
        let want = census(GraphShape::Fanout, ShapeScale::Small, IndexCounts::NONE, 1).unwrap();
        assert_eq!(report.nodes, want.nodes);
        assert_eq!(report.rels, want.relationships);
        assert!(report.topology_logical_bytes < report.total_logical_bytes);
    }

    #[test]
    fn dataset_kind_serializes_to_the_report_spelling() {
        assert_eq!(
            serde_json::to_string(&DatasetKind::Store).unwrap(),
            "\"store\""
        );
        assert_eq!(
            serde_json::to_string(&DatasetKind::SyntheticShape).unwrap(),
            "\"synthetic-shape\""
        );
    }

    #[test]
    fn empty_config_is_an_error() {
        let cfg = BenchConfig {
            stores: vec![],
            shapes: vec![],
            shape_logical_bytes: 1024 * 1024,
            attributes: 1,
            rounds: 1,
            out: None,
            repro_command: String::new(),
        };
        assert!(run(&cfg).unwrap_err().contains("nothing to benchmark"));
    }
}
