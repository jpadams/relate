//! Streaming pull (ADR-0158): one process, one checkpoint, rows materialized
//! in ADR-0155 batches as they arrive off the HTTP stream.

use std::collections::HashMap;

use relate::{Connection, Database};
use serde_json::Value as Json;

use crate::extract::SubgraphAccumulator;
use crate::materialize::{load_nodes, load_rels, with_txn};
use crate::{http, LoadError, LoadOpts, LoadStats};

/// Rows buffered before a flush (also the WAL transaction bound: one flush
/// is one transaction, one fsync per this many rows).
const FLUSH_ROWS: usize = 1_000;

/// One pull's streaming consumer: rows arrive off the HTTP stream, elements
/// dedupe across the whole load, and pending elements materialize in batches
/// so neither the HTTP body nor the extracted graph is ever held whole.
pub struct StreamingLoad<'a> {
    conn: &'a mut Connection,
    source: String,
    /// Interned subgraph id → local node id, across the whole load: a later
    /// statement's rel endpoints resolve through it.
    id_map: HashMap<i64, i64>,
    acc: SubgraphAccumulator,
    stats: LoadStats,
}

impl<'a> StreamingLoad<'a> {
    pub fn new(conn: &'a mut Connection, source: String) -> Self {
        Self {
            conn,
            source,
            id_map: HashMap::new(),
            acc: SubgraphAccumulator::default(),
            stats: LoadStats::default(),
        }
    }

    /// Ingest streamed `data.values` rows; flush when a batch is pending.
    /// A value the engine cannot represent fails here, before anything is
    /// written (ADR-0181 item 7).
    pub fn push_rows(&mut self, rows: &[Json]) -> Result<(), LoadError> {
        for row in rows {
            self.acc.push_row(row)?;
        }
        if self.acc.pending() >= FLUSH_ROWS {
            self.flush()?;
        }
        Ok(())
    }

    /// Flush everything; a rel whose endpoints never arrived in this load is
    /// still pending in the accumulator and drops with `self` — dangling,
    /// like the whole-subgraph path always did.
    pub fn finish(mut self) -> Result<LoadStats, LoadError> {
        self.flush()?;
        Ok(self.stats)
    }

    fn flush(&mut self) -> Result<(), LoadError> {
        let (nodes, rels) = self.acc.drain();
        // Rels partition AFTER the node load: a rel's endpoints resolve
        // through nodes in this same flush. Unresolved rels may resolve in a
        // later batch of the load — retained in the accumulator.
        let pending = with_txn(self.conn, |conn| {
            load_nodes(
                conn,
                &nodes,
                &self.source,
                &mut self.id_map,
                &mut self.stats,
            )?;
            let (ready, pending): (Vec<_>, Vec<_>) = rels.into_iter().partition(|rel| {
                self.id_map.contains_key(&rel.start) && self.id_map.contains_key(&rel.end)
            });
            load_rels(
                conn,
                &ready,
                &self.source,
                &mut self.id_map,
                &mut self.stats,
            )?;
            Ok(pending)
        })?;
        for rel in pending {
            self.acc.push_rel(rel);
        }
        Ok(())
    }
}

/// Execute every statement of a multi-statement `--query` against the source,
/// materializing streamed rows through one [`StreamingLoad`]; one checkpoint
/// at the end. Refuses a non-empty destination (ADR-0158).
pub fn load_streaming(opts: &LoadOpts, db: &Database) -> Result<LoadStats, LoadError> {
    let statements = crate::shape::split_statements(&opts.query)?;
    for statement in &statements {
        crate::shape::assert_graph_shape(statement)?;
    }
    refuse_non_empty(db)?;
    let source = http::identity_source(&opts.credentials)?;
    let mut conn = db.connect().map_err(|e| LoadError::Engine(e.to_string()))?;
    let mut load = StreamingLoad::new(&mut conn, source);
    for statement in &statements {
        // The pull is the one caller that asks the source for typed JSON
        // (ADR-0181): spatial and temporal property values must arrive with
        // their `$type` tags instead of as strings.
        http::run_query_streaming_typed(
            &opts.credentials,
            statement,
            &opts.params,
            opts.allow_plaintext,
            &mut |rows: &[Json]| load.push_rows(rows),
        )?;
    }
    let mut stats = load.finish()?;
    // The data is loaded; now carry the source schema (ADR-0201). A source
    // that cannot answer the SHOW queries or a DDL the engine rejects never
    // fails the pull — the mismatch is counted and reported on stderr.
    stats.schema = crate::schema::pull_schema(&opts.credentials, opts.allow_plaintext, &mut conn);
    drop(conn);
    db.checkpoint()
        .map_err(|e| LoadError::Engine(e.to_string()))?;
    Ok(stats)
}

/// A pull writes into an empty database only (ADR-0158): a non-empty
/// destination is a typed refusal, not a merge. The gate fails closed: a
/// count query that returns no integer row is an engine error, never a
/// silent "empty".
fn refuse_non_empty(db: &Database) -> Result<(), LoadError> {
    let mut conn = db.connect().map_err(|e| LoadError::Engine(e.to_string()))?;
    for count in [
        "MATCH (n) RETURN count(n)",
        "MATCH ()-[r]->() RETURN count(r)",
    ] {
        let r = conn
            .query(count)
            .map_err(|e| LoadError::Engine(e.to_string()))?;
        let n = r
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(relate::Value::as_int)
            .ok_or_else(|| LoadError::Engine(format!("`{count}` returned no count row")))?;
        if n > 0 {
            return Err(LoadError::NonEmpty);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn flush_rows_bound_matches_the_batched_write_bound() {
        assert_eq!(super::FLUSH_ROWS, crate::materialize::BATCH_ROWS);
    }
    // The loopback end-to-end tests live in `tests/streaming.rs`: the shared
    // fixture's `loopback_opts` returns the *dependency* build's `LoadOpts`,
    // which is a different type from the lib-under-test's inside a unit test.
}
