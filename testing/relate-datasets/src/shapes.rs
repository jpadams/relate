//! Deterministic graph shapes for logical-memory pins and the RSS loadtest.
//!
//! [`statements`] emits a fixed list for the `Small` pins. The loadtest uses
//! [`FillCursor`], which emits the same batch shapes and keeps going until
//! the caller stops at the memory limit. The same Cypher runs in-process
//! (`memory_usage`) and inside a separate `relate-bolt` process. Counts,
//! string lengths, and index DDL are fixed so a given
//! `(shape, scale, indexes, attributes)` always charges the same logical bytes.
//!
//! This module owns the shape types, their CLI spellings, and the Cypher
//! statement generators; the closed-form logical-byte arithmetic that turns a
//! target into node counts lives in the sibling [`crate::shape_size`], whose
//! [`census`] is re-exported below.

pub use crate::shape_size::census;

use crate::shape_size::{
    dense_counts, fanout_counts, flat_nodes, integer_key, point_key, ring_nodes, target_bytes,
    text_key, INDEXED_STRING,
};

/// One synthetic graph. `Base` is empty. `Ring` is a directed cycle.
/// `Fanout` is hubs and leaves. `Flat` is nodes with no relationships.
/// `DenseRing` is a directed cycle where each node also skips ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphShape {
    Base,
    Ring,
    Fanout,
    Flat,
    DenseRing,
}

/// `Small` is the `cargo test` pin. Every filled shape has 1,000 nodes.
/// `Large` is the closed-form census for a logical size. The RSS loadtest
/// does not stop at that count; it writes until the database hits the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeScale {
    Small,
    Large { logical_bytes: u64 },
}

/// How many attributes of each index type a node carries.
///
/// Text and point counts add that many properties on top of the integer
/// `attributes`. Range counts index the first integer properties and must
/// not exceed `attributes`. Zero means that index type is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct IndexCounts {
    pub text: u32,
    pub point: u32,
    pub range: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeCensus {
    pub nodes: u64,
    pub relationships: u64,
}

/// Rows per statement batch for every filled shape.
const BATCH: u32 = 2_000;

/// `(start, last)` inclusive id windows of at most [`BATCH`].
fn batch_ranges(total: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut start = 0u32;
    std::iter::from_fn(move || {
        if start >= total {
            return None;
        }
        let end = (start + BATCH).min(total);
        let last = end - 1;
        let window = (start, last);
        start = end;
        Some(window)
    })
}
/// Integer properties on every node. The first is `id`. Default is one.
pub const DEFAULT_NODE_ATTRIBUTES: u32 = 1;

impl GraphShape {
    /// Memory-shape report spelling (`dense ring` with a space).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Ring => "ring",
            Self::Fanout => "fanout",
            Self::Flat => "flat",
            Self::DenseRing => "dense ring",
        }
    }
}

impl std::fmt::Display for GraphShape {
    /// Kebab-case CLI spelling (`dense-ring`); the canonical pair of
    /// [`FromStr`](std::str::FromStr) and the form used in `shape-<name>`
    /// dataset names.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Base => "base",
            Self::Ring => "ring",
            Self::Fanout => "fanout",
            Self::Flat => "flat",
            Self::DenseRing => "dense-ring",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for GraphShape {
    type Err = String;

    /// Parses the kebab-case CLI spellings [`Display`](std::fmt::Display)
    /// emits, minus `base` (not a loadable shape on the CLI).
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "ring" => Ok(Self::Ring),
            "fanout" => Ok(Self::Fanout),
            "flat" => Ok(Self::Flat),
            "dense-ring" => Ok(Self::DenseRing),
            other => Err(format!(
                "unknown shape {other:?}; expected ring, fanout, flat, or dense-ring"
            )),
        }
    }
}

impl IndexCounts {
    pub const NONE: Self = Self {
        text: 0,
        point: 0,
        range: 0,
    };

    /// Compare key. `none` when every count is zero, otherwise
    /// `text2+point1+range1` with zero counts omitted, in that order.
    pub fn token(self) -> String {
        if self == Self::NONE {
            return "none".into();
        }
        let mut parts = Vec::new();
        if self.text > 0 {
            parts.push(format!("text{}", self.text));
        }
        if self.point > 0 {
            parts.push(format!("point{}", self.point));
        }
        if self.range > 0 {
            parts.push(format!("range{}", self.range));
        }
        parts.join("+")
    }
}

impl ShapeScale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Large { .. } => "large",
        }
    }
}

/// Shapes whose process RSS the loadtest records, in report order.
/// `base` is the empty server subtracted from the others. Index counts are
/// run-wide and are not part of this list.
pub fn rss_matrix() -> [GraphShape; 5] {
    [
        GraphShape::Base,
        GraphShape::Flat,
        GraphShape::Fanout,
        GraphShape::Ring,
        GraphShape::DenseRing,
    ]
}

/// Cypher that builds `shape` at `scale` after optional index DDL.
/// Statements are independent autocommit queries. Writes return nothing.
/// `base` emits nothing, including no index DDL, so its RSS stays the floor.
pub fn statements(
    shape: GraphShape,
    scale: ShapeScale,
    indexes: IndexCounts,
    attributes: u32,
) -> Result<Vec<String>, String> {
    validate(attributes, indexes, scale)?;
    let mut out = index_statements(shape, indexes);
    match shape {
        GraphShape::Base => {}
        GraphShape::Ring => out.extend(ring_statements(
            ring_nodes(scale, attributes, indexes)?,
            attributes,
            indexes,
        )),
        GraphShape::Fanout => {
            let (hubs, leaves) = fanout_counts(scale, attributes, indexes)?;
            out.extend(fanout_statements(hubs, leaves, attributes, indexes));
        }
        GraphShape::Flat => out.extend(flat_statements(
            flat_nodes(scale, attributes, indexes)?,
            attributes,
            indexes,
        )),
        GraphShape::DenseRing => {
            let (nodes, degree) = dense_counts(scale, attributes, indexes)?;
            out.extend(dense_statements(nodes, degree, attributes, indexes));
        }
    }
    Ok(out)
}

pub(crate) fn validate(
    attributes: u32,
    indexes: IndexCounts,
    scale: ShapeScale,
) -> Result<(), String> {
    if attributes == 0 {
        return Err("attributes per node must be at least 1".into());
    }
    if indexes.range > attributes {
        return Err(format!(
            "range indexes ({}) exceed attributes per node ({})",
            indexes.range, attributes
        ));
    }
    if let ShapeScale::Large { logical_bytes } = scale {
        target_bytes(logical_bytes)?;
    }
    Ok(())
}

/// Default loadtest target: 32 MiB of logical graph memory per filled shape.
pub const DEFAULT_LOGICAL_TARGET_BYTES: u64 = 32 * 1024 * 1024;

fn labels(shape: GraphShape) -> &'static [&'static str] {
    match shape {
        GraphShape::Base => &[],
        GraphShape::Flat => &["Flat"],
        GraphShape::Ring => &["Ring"],
        GraphShape::DenseRing => &["DenseRing"],
        GraphShape::Fanout => &["Hub", "Leaf"],
    }
}

type IndexDdl = (u32, &'static str, fn(u32) -> String);

fn index_statements(shape: GraphShape, indexes: IndexCounts) -> Vec<String> {
    let kinds: [IndexDdl; 3] = [
        (indexes.text, "TEXT", text_key),
        (indexes.point, "POINT", point_key),
        (indexes.range, "RANGE", integer_key),
    ];
    let mut out = Vec::new();
    for label in labels(shape) {
        for (count, keyword, key_of) in kinds {
            for i in 0..count {
                let key = key_of(i);
                out.push(format!(
                    "CREATE {keyword} INDEX {label}_{key} FOR (n:{label}) ON (n.{key})"
                ));
            }
        }
    }
    out
}

/// `id` plus `a1`..`a{attributes-1}`, then the indexed text and point values.
fn node_props(value: &str, attributes: u32, indexes: IndexCounts) -> String {
    let mut parts = Vec::new();
    for n in 0..attributes {
        let key = integer_key(n);
        parts.push(format!("{key}: {value}"));
    }
    for i in 0..indexes.text {
        parts.push(format!("{key}: '{INDEXED_STRING}'", key = text_key(i)));
    }
    for i in 0..indexes.point {
        parts.push(format!(
            "{key}: point({{srid: 7203, x: 1.0, y: 2.0}})",
            key = point_key(i)
        ));
    }
    parts.join(", ")
}

fn ring_statements(nodes: u32, attributes: u32, indexes: IndexCounts) -> Vec<String> {
    let props = node_props("i", attributes, indexes);
    let mut out = Vec::new();
    for (start, last) in batch_ranges(nodes) {
        out.push(format!(
            "UNWIND range({start}, {last}) AS i CREATE (:Ring {{{props}}})"
        ));
        if last > start {
            out.push(format!(
                "MATCH (c:Ring) WHERE c.id >= {start} AND c.id <= {last} \
                 WITH c ORDER BY c.id \
                 WITH collect(c) AS nodes \
                 UNWIND range(0, size(nodes) - 2) AS i \
                 WITH nodes[i] AS a, nodes[i + 1] AS b \
                 CREATE (a)-[:NEXT]->(b)"
            ));
        }
        if start > 0 {
            let prev = start - 1;
            out.push(format!(
                "MATCH (a:Ring {{id: {prev}}}), (b:Ring {{id: {start}}}) \
                 CREATE (a)-[:NEXT]->(b)"
            ));
        }
    }
    if nodes >= 2 {
        let last = nodes - 1;
        out.push(format!(
            "MATCH (a:Ring {{id: {last}}}), (b:Ring {{id: 0}}) CREATE (a)-[:NEXT]->(b)"
        ));
    }
    out
}

fn fanout_statements(hubs: u32, leaves: u32, attributes: u32, indexes: IndexCounts) -> Vec<String> {
    let hub_props = node_props("h", attributes, indexes);
    let mut out = Vec::new();
    for (start, last) in batch_ranges(hubs) {
        out.push(format!(
            "UNWIND range({start}, {last}) AS h CREATE (:Hub {{{hub_props}}})"
        ));
    }
    // One hub per statement. Crossing every hub with one leaf window made
    // each statement `hubs × batch` rows, which the other shapes never do.
    for h in 0..hubs {
        let leaf_props = node_props(&format!("{h} * {leaves} + l"), attributes, indexes);
        for (start, last) in batch_ranges(leaves) {
            out.push(format!(
                "MATCH (hub:Hub {{id: {h}}}) \
                 UNWIND range({start}, {last}) AS l \
                 CREATE (hub)-[:LEAF]->(:Leaf {{{leaf_props}}})"
            ));
        }
    }
    out
}

fn flat_statements(nodes: u32, attributes: u32, indexes: IndexCounts) -> Vec<String> {
    let props = node_props("i", attributes, indexes);
    batch_ranges(nodes)
        .map(|(start, last)| {
            format!("UNWIND range({start}, {last}) AS i CREATE (:Flat {{{props}}})")
        })
        .collect()
}

fn dense_statements(nodes: u32, degree: u32, attributes: u32, indexes: IndexCounts) -> Vec<String> {
    let props = node_props("i", attributes, indexes);
    let mut out: Vec<String> = batch_ranges(nodes)
        .map(|(start, last)| {
            format!("UNWIND range({start}, {last}) AS i CREATE (:DenseRing {{{props}}})")
        })
        .collect();
    // Source windows of `BATCH / degree` keep each statement at or under
    // BATCH relationship rows, the same cap as a ring batch.
    let width = if degree == 0 {
        nodes
    } else {
        (BATCH / degree).max(1)
    };
    let mut start = 0u32;
    while start < nodes {
        let last = (start + width).min(nodes) - 1;
        out.push(format!(
            "UNWIND range({start}, {last}) AS i \
             UNWIND range(1, {degree}) AS d \
             WITH i, (i + d) % {nodes} AS j \
             MATCH (a:DenseRing {{id: i}}), (b:DenseRing {{id: j}}) \
             CREATE (a)-[:LINK]->(b)"
        ));
        start = last + 1;
    }
    out
}

/// Large-scale fanout hubs. The loadtest grows leaves under these hubs
/// until the memory limit, rather than picking a leaf count up front.
const FILL_HUBS: u32 = 16;
/// Large-scale dense-ring out-degree. Each window is created and linked in
/// one statement, so a rejected batch cannot leave nodes without edges.
const FILL_DEGREE: u32 = 20;

/// One autocommit batch of a filled shape, for the RSS loadtest.
///
/// Index DDL comes first. Later calls keep emitting data batches; the caller
/// stops when the database reports the memory limit. [`ack`][FillCursor::ack]
/// counts what committed. [`nack`][FillCursor::nack] drops the batch that
/// was rejected. `base` yields nothing.
#[derive(Clone)]
pub(crate) struct FillCursor {
    shape: GraphShape,
    attributes: u32,
    indexes: IndexCounts,
    index_stmts: Vec<String>,
    index_at: usize,
    pos: Pos,
    pending: Option<Pending>,
    pub(crate) nodes: u64,
    pub(crate) rels: u64,
}

#[derive(Clone)]
struct Pending {
    pos: Pos,
    index_at: usize,
    kind: TickKind,
}

#[derive(Clone, Copy)]
enum TickKind {
    Index,
    Nodes(u32),
    Rels(u32),
    NodesAndRels { nodes: u32, rels: u32 },
}

#[derive(Clone)]
struct Pos {
    next: u32,
    ring: RingStep,
    hubs_done: bool,
    hub: u32,
    leaf_at: [u32; FILL_HUBS as usize],
}

impl Default for Pos {
    fn default() -> Self {
        Self {
            next: 0,
            ring: RingStep::Create,
            hubs_done: false,
            hub: 0,
            leaf_at: [0; FILL_HUBS as usize],
        }
    }
}

#[derive(Clone)]
enum RingStep {
    Create,
    Link { start: u32, last: u32 },
    Bridge { start: u32, last: u32 },
}

impl FillCursor {
    pub(crate) fn new(shape: GraphShape, attributes: u32, indexes: IndexCounts) -> Self {
        Self {
            shape,
            attributes,
            indexes,
            index_stmts: index_statements(shape, indexes),
            index_at: 0,
            pos: Pos::default(),
            pending: None,
            nodes: 0,
            rels: 0,
        }
    }

    /// The next statement, or `None` when the shape cannot grow (`base`).
    pub(crate) fn next_statement(&mut self) -> Result<Option<String>, String> {
        if self.pending.is_some() {
            return Err("fill cursor has an unacked statement".into());
        }
        if self.index_at < self.index_stmts.len() {
            let statement = self.index_stmts[self.index_at].clone();
            self.pending = Some(Pending {
                pos: self.pos.clone(),
                index_at: self.index_at,
                kind: TickKind::Index,
            });
            self.index_at += 1;
            return Ok(Some(statement));
        }
        if self.shape == GraphShape::Base {
            return Ok(None);
        }
        let saved = self.pos.clone();
        let (statement, kind) = self.emit_data()?;
        self.pending = Some(Pending {
            pos: saved,
            index_at: self.index_at,
            kind,
        });
        Ok(Some(statement))
    }

    pub(crate) fn ack(&mut self) -> Result<(), String> {
        let pending = self
            .pending
            .take()
            .ok_or("fill cursor has nothing to ack")?;
        match pending.kind {
            TickKind::Index => {}
            TickKind::Nodes(n) => self.nodes += u64::from(n),
            TickKind::Rels(n) => self.rels += u64::from(n),
            TickKind::NodesAndRels { nodes, rels } => {
                self.nodes += u64::from(nodes);
                self.rels += u64::from(rels);
            }
        }
        Ok(())
    }

    pub(crate) fn nack(&mut self) -> Result<(), String> {
        let pending = self
            .pending
            .take()
            .ok_or("fill cursor has nothing to roll back")?;
        self.pos = pending.pos;
        self.index_at = pending.index_at;
        Ok(())
    }

    /// Close a ring whose committed batches are each fully linked.
    ///
    /// Absent when the rejected statement was a link or a bridge: that batch
    /// of nodes is already committed and is not part of the cycle.
    pub(crate) fn ring_wrap_statement(&self) -> Option<String> {
        if self.shape != GraphShape::Ring || self.pending.is_some() {
            return None;
        }
        if !matches!(self.pos.ring, RingStep::Create) || self.nodes < 2 {
            return None;
        }
        let last = self.nodes - 1;
        Some(format!(
            "MATCH (a:Ring {{id: {last}}}), (b:Ring {{id: 0}}) CREATE (a)-[:NEXT]->(b)"
        ))
    }

    pub(crate) fn note_wrap(&mut self) {
        self.rels += 1;
    }

    fn emit_data(&mut self) -> Result<(String, TickKind), String> {
        let attributes = self.attributes;
        let indexes = self.indexes;
        let mut pos = self.pos.clone();
        let out = match self.shape {
            GraphShape::Flat => emit_flat(&mut pos, attributes, indexes)?,
            GraphShape::Ring => emit_ring(&mut pos, attributes, indexes)?,
            GraphShape::Fanout => emit_fanout(&mut pos, attributes, indexes)?,
            GraphShape::DenseRing => emit_dense(&mut pos, attributes, indexes)?,
            GraphShape::Base => unreachable!("base has no data statements"),
        };
        self.pos = pos;
        Ok(out)
    }
}

fn id_window(start: u32) -> Result<(u32, u32), String> {
    let last = start
        .checked_add(BATCH - 1)
        .ok_or("node id overflow while filling")?;
    Ok((start, last))
}

fn advance(last: u32) -> Result<u32, String> {
    last.checked_add(1)
        .ok_or_else(|| "node id overflow while filling".into())
}

fn emit_flat(
    pos: &mut Pos,
    attributes: u32,
    indexes: IndexCounts,
) -> Result<(String, TickKind), String> {
    let (start, last) = id_window(pos.next)?;
    let props = node_props("i", attributes, indexes);
    let statement = format!("UNWIND range({start}, {last}) AS i CREATE (:Flat {{{props}}})");
    pos.next = advance(last)?;
    Ok((statement, TickKind::Nodes(BATCH)))
}

fn emit_ring(
    pos: &mut Pos,
    attributes: u32,
    indexes: IndexCounts,
) -> Result<(String, TickKind), String> {
    match pos.ring {
        RingStep::Create => {
            let (start, last) = id_window(pos.next)?;
            let props = node_props("i", attributes, indexes);
            let statement =
                format!("UNWIND range({start}, {last}) AS i CREATE (:Ring {{{props}}})");
            pos.ring = RingStep::Link { start, last };
            Ok((statement, TickKind::Nodes(last - start + 1)))
        }
        RingStep::Link { start, last } => {
            let statement = format!(
                "MATCH (c:Ring) WHERE c.id >= {start} AND c.id <= {last} \
                 WITH c ORDER BY c.id \
                 WITH collect(c) AS nodes \
                 UNWIND range(0, size(nodes) - 2) AS i \
                 WITH nodes[i] AS a, nodes[i + 1] AS b \
                 CREATE (a)-[:NEXT]->(b)"
            );
            let links = last - start;
            if start > 0 {
                pos.ring = RingStep::Bridge { start, last };
            } else {
                pos.next = advance(last)?;
                pos.ring = RingStep::Create;
            }
            Ok((statement, TickKind::Rels(links)))
        }
        RingStep::Bridge { start, last } => {
            let prev = start - 1;
            let statement = format!(
                "MATCH (a:Ring {{id: {prev}}}), (b:Ring {{id: {start}}}) \
                 CREATE (a)-[:NEXT]->(b)"
            );
            pos.next = advance(last)?;
            pos.ring = RingStep::Create;
            Ok((statement, TickKind::Rels(1)))
        }
    }
}

fn emit_fanout(
    pos: &mut Pos,
    attributes: u32,
    indexes: IndexCounts,
) -> Result<(String, TickKind), String> {
    if !pos.hubs_done {
        let last = FILL_HUBS - 1;
        let props = node_props("h", attributes, indexes);
        let statement = format!("UNWIND range(0, {last}) AS h CREATE (:Hub {{{props}}})");
        pos.hubs_done = true;
        return Ok((statement, TickKind::Nodes(FILL_HUBS)));
    }
    let hub = pos.hub;
    let start = pos.leaf_at[hub as usize];
    let (_, last) = id_window(start)?;
    let props = node_props("l", attributes, indexes);
    let statement = format!(
        "MATCH (hub:Hub {{id: {hub}}}) \
         UNWIND range({start}, {last}) AS l \
         CREATE (hub)-[:LEAF]->(:Leaf {{{props}}})"
    );
    pos.leaf_at[hub as usize] = advance(last)?;
    pos.hub = (hub + 1) % FILL_HUBS;
    Ok((
        statement,
        TickKind::NodesAndRels {
            nodes: BATCH,
            rels: BATCH,
        },
    ))
}

fn emit_dense(
    pos: &mut Pos,
    attributes: u32,
    indexes: IndexCounts,
) -> Result<(String, TickKind), String> {
    // One statement creates the window and links it. A capacity failure rolls
    // both back, so every committed node has its `FILL_DEGREE` edges.
    let start = pos.next;
    let width = (BATCH / FILL_DEGREE).max(1);
    let last = start
        .checked_add(width - 1)
        .ok_or("node id overflow while filling")?;
    let nodes = u64::from(last) + 1;
    let props = node_props("i", attributes, indexes);
    let statement = format!(
        "UNWIND range({start}, {last}) AS i \
         CREATE (:DenseRing {{{props}}}) \
         WITH count(*) AS created \
         UNWIND range({start}, {last}) AS i \
         UNWIND range(1, {FILL_DEGREE}) AS d \
         WITH i, (i + d) % {nodes} AS j \
         MATCH (a:DenseRing {{id: i}}), (b:DenseRing {{id: j}}) \
         CREATE (a)-[:LINK]->(b)"
    );
    pos.next = advance(last)?;
    Ok((
        statement,
        TickKind::NodesAndRels {
            nodes: width,
            rels: width * FILL_DEGREE,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none_small(shape: GraphShape) -> ShapeCensus {
        census(
            shape,
            ShapeScale::Small,
            IndexCounts::NONE,
            DEFAULT_NODE_ATTRIBUTES,
        )
        .unwrap()
    }

    #[test]
    fn statements_are_stable_and_indexes_apply_to_every_filled_shape() {
        let once = statements(
            GraphShape::Ring,
            ShapeScale::Small,
            IndexCounts::NONE,
            DEFAULT_NODE_ATTRIBUTES,
        )
        .unwrap();
        let twice = statements(
            GraphShape::Ring,
            ShapeScale::Small,
            IndexCounts::NONE,
            DEFAULT_NODE_ATTRIBUTES,
        )
        .unwrap();
        assert_eq!(once, twice);
        assert!(once
            .iter()
            .any(|s| s.contains("id: 999") && s.contains("id: 0")));
        assert!(once.iter().all(|s| !s.contains("CREATE TEXT INDEX")));
        assert!(statements(GraphShape::Flat, ShapeScale::Small, IndexCounts::NONE, 0).is_err());
        assert!(statements(
            GraphShape::Flat,
            ShapeScale::Small,
            IndexCounts {
                text: 0,
                point: 0,
                range: 2
            },
            1
        )
        .is_err());

        let indexed = IndexCounts {
            text: 1,
            point: 1,
            range: 1,
        };
        let ring = statements(
            GraphShape::Ring,
            ShapeScale::Small,
            indexed,
            DEFAULT_NODE_ATTRIBUTES,
        )
        .unwrap();
        assert!(ring.iter().any(|s| s.contains("CREATE TEXT INDEX Ring_s0")));
        assert!(ring
            .iter()
            .any(|s| s.contains("CREATE POINT INDEX Ring_loc0")));
        assert!(ring
            .iter()
            .any(|s| s.contains("CREATE RANGE INDEX Ring_id")));
        assert!(ring.iter().any(|s| s.contains("s0: 'abcdefghijklmnop'")));
        assert!(statements(
            GraphShape::Base,
            ShapeScale::Small,
            indexed,
            DEFAULT_NODE_ATTRIBUTES,
        )
        .unwrap()
        .is_empty());

        let fanout = statements(
            GraphShape::Fanout,
            ShapeScale::Small,
            IndexCounts {
                text: 1,
                point: 0,
                range: 0,
            },
            DEFAULT_NODE_ATTRIBUTES,
        )
        .unwrap();
        assert!(fanout
            .iter()
            .any(|s| s.contains("CREATE TEXT INDEX Hub_s0")));
        assert!(fanout
            .iter()
            .any(|s| s.contains("CREATE TEXT INDEX Leaf_s0")));
        assert_eq!(indexed.token(), "text1+point1+range1");
        assert_eq!(IndexCounts::NONE.token(), "none");
    }

    #[test]
    fn census_matches_the_scale_constants() {
        assert_eq!(
            none_small(GraphShape::Base),
            ShapeCensus {
                nodes: 0,
                relationships: 0
            }
        );
        assert_eq!(
            none_small(GraphShape::Ring),
            ShapeCensus {
                nodes: 1_000,
                relationships: 1_000
            }
        );
        assert_eq!(
            none_small(GraphShape::Fanout),
            ShapeCensus {
                nodes: 1_000,
                relationships: 996
            }
        );
        assert_eq!(none_small(GraphShape::Flat).nodes, 1_000);
        assert_eq!(
            none_small(GraphShape::DenseRing),
            ShapeCensus {
                nodes: 1_000,
                relationships: 8_000
            }
        );
        let target = ShapeScale::Large {
            logical_bytes: DEFAULT_LOGICAL_TARGET_BYTES,
        };
        let attrs = DEFAULT_NODE_ATTRIBUTES;
        let none = IndexCounts::NONE;
        assert_eq!(
            census(GraphShape::Ring, target, none, attrs).unwrap(),
            ShapeCensus {
                nodes: 46_603,
                relationships: 46_603
            }
        );
        assert_eq!(
            census(GraphShape::Fanout, target, none, attrs).unwrap(),
            ShapeCensus {
                nodes: 16 + 16 * 3_244,
                relationships: 16 * 3_244
            }
        );
        assert_eq!(
            census(GraphShape::Flat, target, none, attrs).unwrap().nodes,
            121_574
        );
        assert_eq!(
            census(GraphShape::DenseRing, target, none, attrs).unwrap(),
            ShapeCensus {
                nodes: 5_010,
                relationships: 5_010 * 20
            }
        );
        let text = IndexCounts {
            text: 1,
            point: 0,
            range: 0,
        };
        assert_eq!(
            census(GraphShape::Flat, target, text, attrs).unwrap().nodes,
            15_190
        );
        assert!(census(GraphShape::Ring, target, text, attrs).unwrap().nodes < 46_603);
        assert!(
            census(GraphShape::Fanout, target, text, attrs)
                .unwrap()
                .nodes
                < 16 + 16 * 3_244
        );
        assert!(
            census(GraphShape::DenseRing, target, text, attrs)
                .unwrap()
                .nodes
                < 5_010
        );
        let half = ShapeScale::Large {
            logical_bytes: DEFAULT_LOGICAL_TARGET_BYTES / 2,
        };
        let ring_half = census(GraphShape::Ring, half, none, attrs).unwrap().nodes;
        assert_eq!(ring_half, 23_302);
        assert!(census(
            GraphShape::Ring,
            ShapeScale::Large { logical_bytes: 0 },
            none,
            attrs
        )
        .is_err());
        let wider = census(GraphShape::Ring, target, none, 2).unwrap().nodes;
        assert_eq!(wider, 42_799);
    }

    /// Fanout used to cross every hub with one leaf window (`16 × 2,000`
    /// rows). Dense ring used to link the whole graph in one statement.
    /// Both now stay within the same 2,000-row cap as flat and ring.
    #[test]
    fn relationship_batches_match_the_node_batch() {
        let target = ShapeScale::Large {
            logical_bytes: DEFAULT_LOGICAL_TARGET_BYTES,
        };
        let none = IndexCounts::NONE;
        let attrs = DEFAULT_NODE_ATTRIBUTES;

        let dense = statements(GraphShape::DenseRing, target, none, attrs).unwrap();
        let links: Vec<_> = dense.iter().filter(|s| s.contains("[:LINK]")).collect();
        assert!(
            links.len() > 1,
            "dense links must be split across statements"
        );
        for statement in &links {
            assert!(!statement.contains("collect("));
            let rows = unwind_width(statement, "i") * unwind_width(statement, "d");
            assert!(rows <= u64::from(BATCH), "{statement}");
        }

        let fanout = statements(GraphShape::Fanout, target, none, attrs).unwrap();
        let leaves: Vec<_> = fanout.iter().filter(|s| s.contains("[:LEAF]")).collect();
        assert!(leaves.len() > 16, "each hub needs its own leaf batches");
        for statement in &leaves {
            assert!(
                !statement.contains("UNWIND range(0, 15)"),
                "one statement must not cross every hub"
            );
            assert!(unwind_width(statement, "l") <= u64::from(BATCH));
        }
    }

    fn unwind_width(statement: &str, alias: &str) -> u64 {
        let marker = format!("AS {alias}");
        let head = statement
            .split(&marker)
            .next()
            .and_then(|prefix| prefix.rsplit("UNWIND range(").next())
            .unwrap_or_else(|| panic!("no UNWIND for {alias} in {statement}"));
        let (start, last) = head
            .trim()
            .trim_end_matches(')')
            .split_once(", ")
            .unwrap_or_else(|| panic!("range in {head}"));
        let start: u64 = start.parse().unwrap();
        let last: u64 = last.parse().unwrap();
        last - start + 1
    }

    #[test]
    fn fill_cursor_keeps_emitting_the_large_batch_shape() {
        let mut base = FillCursor::new(GraphShape::Base, 1, IndexCounts::NONE);
        assert!(base.next_statement().unwrap().is_none());

        let mut flat = FillCursor::new(GraphShape::Flat, 1, IndexCounts::NONE);
        let first = flat.next_statement().unwrap().unwrap();
        assert!(first.contains("range(0, 1999)") && first.contains(":Flat"));
        flat.ack().unwrap();
        let second = flat.next_statement().unwrap().unwrap();
        assert!(second.contains("range(2000, 3999)"));
        flat.nack().unwrap();
        assert_eq!(flat.next_statement().unwrap().unwrap(), second);

        let mut ring = FillCursor::new(GraphShape::Ring, 1, IndexCounts::NONE);
        ring.next_statement().unwrap().unwrap();
        ring.ack().unwrap();
        assert!(ring.ring_wrap_statement().is_none());
        let link = ring.next_statement().unwrap().unwrap();
        assert!(link.contains("[:NEXT]"));
        ring.ack().unwrap();
        assert_eq!((ring.nodes, ring.rels), (2_000, 1_999));
        let wrap = ring.ring_wrap_statement().unwrap();
        assert!(wrap.contains("id: 1999") && wrap.contains("id: 0"));

        let mut dense = FillCursor::new(GraphShape::DenseRing, 1, IndexCounts::NONE);
        let window = dense.next_statement().unwrap().unwrap();
        assert!(window.contains("range(0, 99)"));
        assert!(window.contains("% 100") && window.contains("range(1, 20)"));
        dense.ack().unwrap();
        assert_eq!((dense.nodes, dense.rels), (100, 2_000));

        let mut fanout = FillCursor::new(GraphShape::Fanout, 1, IndexCounts::NONE);
        let hubs = fanout.next_statement().unwrap().unwrap();
        assert!(hubs.contains("range(0, 15)") && hubs.contains(":Hub"));
        fanout.ack().unwrap();
        let leaves = fanout.next_statement().unwrap().unwrap();
        assert!(leaves.contains(":Leaf") && leaves.contains("range(0, 1999)"));
        fanout.ack().unwrap();
        assert_eq!((fanout.nodes, fanout.rels), (16 + 2_000, 2_000));

        let indexed = IndexCounts {
            text: 1,
            point: 0,
            range: 0,
        };
        let mut text = FillCursor::new(GraphShape::Flat, 1, indexed);
        let ddl = text.next_statement().unwrap().unwrap();
        assert!(ddl.contains("CREATE TEXT INDEX Flat_s0"));
        text.ack().unwrap();
        assert_eq!(text.nodes, 0);
    }

    #[test]
    fn shape_cli_spelling_roundtrips() {
        for (shape, kebab) in [
            (GraphShape::Ring, "ring"),
            (GraphShape::Fanout, "fanout"),
            (GraphShape::Flat, "flat"),
            (GraphShape::DenseRing, "dense-ring"),
        ] {
            assert_eq!(shape.to_string(), kebab);
            assert_eq!(kebab.parse::<GraphShape>().unwrap(), shape);
        }
        assert!("base".parse::<GraphShape>().is_err());
        assert!("dense ring".parse::<GraphShape>().is_err());
    }
}
