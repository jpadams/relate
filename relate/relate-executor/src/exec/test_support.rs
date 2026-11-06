//! Shared test helpers: lower Cypher with the product front-end and run it
//! through `execute_read_at` at a freshly pinned snapshot, or through
//! `execute` (or `execute_unvalidated`) on a write scratch whose redo a test
//! then commits; and build name-keyed or index-keyed graphs.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::thread::LocalKey;

use relate_core::{NodeId, RelId, Result, Value};
use relate_front::parse_and_lower;
use relate_ir::{Expr, Query};
use relate_planner::plan;

use crate::ctx::{ClauseMetricsRecord, ExecCtx, PlanMetricsSink, QueryCtx};
use crate::graph::Graph;
use crate::overlay::{ExecGraph, WriteScratch};
use crate::row::Row;

use super::{
    execute, execute_read_at, execute_unvalidated, ExecRequest, QueryResult, ReadAdmission,
};

/// The pushdown/limit-cap tests' hub-and-fan graph: a hub `(:A {id: 0})`
/// with `degree` `(:B {k: i})` leaves on `E` rels. `rel_ts` adds the
/// `ts = i` relationship property the ordered cap's shapes read — the
/// one fact `hub_ts_out` ever had over `hub_out` (one builder, not two
/// per-file copies).
/// The hub-and-fan graph with plain `E` rels (the pushdown shapes).
pub(crate) fn hub_out(degree: i64) -> Graph {
    hub_fan(degree, false)
}

/// `hub_out` plus the `ts = i` relationship property the ordered cap's
/// shapes read.
pub(crate) fn hub_ts_out(degree: i64) -> Graph {
    hub_fan(degree, true)
}

fn hub_fan(degree: i64, rel_ts: bool) -> Graph {
    let g = Graph::default();
    g.begin_write().unwrap();
    let hub = g
        .create_node(
            vec!["A".into()],
            BTreeMap::from([("id".into(), Value::Int(0))]),
        )
        .unwrap();
    for i in 0..degree {
        let n = g
            .create_node(
                vec!["B".into()],
                BTreeMap::from([("k".into(), Value::Int(i))]),
            )
            .unwrap();
        let props = if rel_ts {
            BTreeMap::from([("ts".into(), Value::Int(i))])
        } else {
            BTreeMap::new()
        };
        g.create_rel(hub, n, "E".into(), props).unwrap();
    }
    g.commit_write();
    g
}

/// Run an already-lowered `Query` (tests that hand-build or rewrite IR —
/// e.g. the REPEATABLE ELEMENTS unbounded-cap parity test, whose spellings
/// the parser itself rejects) through the same execute path as
/// [`exec_cypher`].
pub(crate) fn exec_lowered(g: &Graph, q: Query, cypher: &str) -> QueryResult {
    try_exec_lowered(g, q, cypher, &BTreeMap::new()).unwrap()
}

/// The fallible form of [`exec_lowered`]: parse/lowering, planning, and
/// execution errors propagate instead of unwrapping, so tests can assert a
/// query's error message whichever phase raises it.
fn try_exec_lowered(
    g: &Graph,
    q: Query,
    cypher: &str,
    params: &BTreeMap<String, Value>,
) -> Result<QueryResult> {
    try_exec_lowered_with_metrics(g, q, cypher, params, None)
}

/// The canonical run scaffold with a plan-metrics sink attached — the
/// one spelling [`try_exec_lowered`] reads with `None` and [`profiled`]
/// with its recorder, so the scaffold (plan, pin, schema, admission) is
/// never re-spelled beside it.
fn try_exec_lowered_with_metrics(
    g: &Graph,
    q: Query,
    cypher: &str,
    params: &BTreeMap<String, Value>,
    plan_metrics: Option<&dyn PlanMetricsSink>,
) -> Result<QueryResult> {
    let p = plan(q, &mut |_| false)?;
    let pin = g.pin_now()?;
    let schema = g.schema_snapshot();
    let mut request = ExecRequest::new(&p, params, cypher);
    request.plan_metrics = plan_metrics;
    execute_read_at(&ReadAdmission {
        graph: g,
        snapshot_ts: pin.ts(),
        schema: &schema,
        request,
        cancel: None,
        ontology_document: None,
    })
}

pub(crate) fn exec_cypher(g: &Graph, cypher: &str) -> QueryResult {
    exec_lowered(g, parse_and_lower(cypher).unwrap(), cypher)
}

/// The fallible form of [`exec_cypher`] (ADR-0175's integer-argument-site
/// tests assert errors raised at plan time and at runtime).
pub(crate) fn try_exec_cypher(g: &Graph, cypher: &str) -> Result<QueryResult> {
    try_exec_cypher_params(g, cypher, &BTreeMap::new())
}

/// [`try_exec_cypher`] with statement parameters.
pub(crate) fn try_exec_cypher_params(
    g: &Graph,
    cypher: &str,
    params: &BTreeMap<String, Value>,
) -> Result<QueryResult> {
    try_exec_lowered(g, parse_and_lower(cypher)?, cypher, params)
}

/// The error message of a run the test expects to fail — the one spelling
/// (`QueryResult` is not `Debug`, so `unwrap_err` does not compile): every
/// error-parity differential (the path fold's, the kernels', the batch
/// chain's, the predicate specialization's) extracts the message through
/// it, never a second local closure.
pub(crate) fn error_of(result: Result<QueryResult>) -> String {
    match result {
        Err(err) => err.to_string(),
        Ok(done) => panic!(
            "the query errors, but it returned a result: {:?}",
            done.rows
        ),
    }
}

/// Run `f` with the thread-local disable-flag `flag` forced on, restoring
/// it on the way out — including on unwind, so a panicking assertion
/// inside the closure cannot leave the path disabled for every later test
/// on the thread. The one spelling of the A/B differential hook (audit
/// finding B3): every `with_X_disabled` in the layer delegates here, each
/// site keeping only its own flag declaration and engagement counter.
pub(crate) fn with_flag_disabled<R>(
    flag: &'static LocalKey<Cell<bool>>,
    f: impl FnOnce() -> R,
) -> R {
    /// Restores the flag on the way out — including on unwind.
    struct Restore {
        flag: &'static LocalKey<Cell<bool>>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            self.flag.with(|cell| cell.set(false));
        }
    }
    flag.with(|cell| cell.set(true));
    let _restore = Restore { flag };
    f()
}

/// Run `cypher` PROFILEd, collecting the clause records the dispatch
/// emits — the one PROFILE harness the exec tests read (the frontier
/// rider's engagement pins, the store-read arm's save/restore pin).
pub(crate) fn profiled(g: &Graph, cypher: &str) -> Vec<ClauseMetricsRecord> {
    struct Recorder(RefCell<Vec<ClauseMetricsRecord>>);
    impl PlanMetricsSink for Recorder {
        fn record_clause(&self, record: ClauseMetricsRecord) {
            self.0.borrow_mut().push(record);
        }
    }
    let recorder = Recorder(RefCell::new(Vec::new()));
    try_exec_lowered_with_metrics(
        g,
        parse_and_lower(cypher).unwrap(),
        cypher,
        &BTreeMap::new(),
        Some(&recorder),
    )
    .unwrap();
    recorder.0.take()
}

/// Run `cypher` on an existing view through `execute`, so the test can
/// inspect per-view state (e.g. the CSV reader's physical-read count) after
/// the run — the view-bound counterpart of [`exec_cypher`].
pub(crate) fn exec_cypher_on(graph: &ExecGraph<'_>, cypher: &str) -> QueryResult {
    try_exec_cypher_on(graph, cypher).unwrap()
}

/// The fallible form of [`exec_cypher_on`].
pub(crate) fn try_exec_cypher_on(graph: &ExecGraph<'_>, cypher: &str) -> Result<QueryResult> {
    let p = plan(parse_and_lower(cypher)?, &mut |_| false)?;
    execute(graph, &ExecRequest::new(&p, &BTreeMap::new(), cypher))
}

/// Runs a `SHOW …` statement through the real catalog path (parse →
/// `execute_catalog` projection) against `graph` and returns the raw
/// rows — the one harness every catalog-show test module reads (the
/// indexes, constraints, and databases key-order pins), never a second
/// pasted scaffold per module.
pub(crate) fn show_catalog_rows(graph: &Graph, query: &str) -> Vec<Row> {
    let view = ExecGraph::read(graph);
    let stmt = relate_parser::parse(query).expect("parse");
    let clause = stmt.clauses.into_iter().next().expect("one clause");
    super::catalog::execute_catalog(
        &view,
        &clause,
        vec![Row::new()],
        &BTreeMap::new(),
        QueryCtx::new(None, query),
    )
    .expect("catalog rows")
}

/// The catalog rows' YIELD-* key-order pin, the one spelling every
/// column-table test reads (the indexes, constraints, and databases
/// pins): against a fresh graph — the pin's documented contract, built
/// here rather than threaded through every call as a ceremony
/// parameter — after `setup` commits (`None` when the statement needs
/// no setup), every `show` statement's row must carry keys in exactly
/// `expected` order (`Row` is an IndexMap and `catalog_result` reads
/// the first row's key order), so `SHOW … YIELD *` exposes the Neo4j
/// ordering — the producer's hand-spelled pair list can never drift
/// from the front table it renders.
pub(crate) fn assert_row_keys_match(setup: Option<&str>, show: &str, expected: &[String]) {
    let graph = Graph::default();
    if let Some(setup) = setup {
        exec_autocommit(&graph, setup).unwrap();
    }
    let rows = show_catalog_rows(&graph, show);
    assert!(!rows.is_empty());
    let expected_keys: Vec<&str> = expected.iter().map(String::as_str).collect();
    for row in &rows {
        let keys: Vec<&str> = row.keys().map(String::as_str).collect();
        assert_eq!(
            keys, expected_keys,
            "the `{show}` rows must carry keys in the front column-table order"
        );
    }
}

/// The cells of result column `name`, in result order, extracted through
/// `value`. The one column reader for the exec tests: a missing column or a
/// cell of the wrong variant panics with the column name.
pub(crate) fn column<T>(
    result: &QueryResult,
    name: &str,
    value: impl Fn(&Value) -> Option<T>,
) -> Vec<T> {
    let index = result
        .columns
        .iter()
        .position(|column| column == name)
        .unwrap_or_else(|| panic!("no `{name}` column in {:?}", result.columns));
    result
        .rows
        .iter()
        .map(|row| {
            value(&row[index]).unwrap_or_else(|| {
                panic!("`{name}` cell is not the expected type: {:?}", row[index])
            })
        })
        .collect()
}

/// The STRING cell of a result column: `Some` for a `Value::String`
/// cell, `None` for any other variant — the one string-cell reader the
/// exec tests pass to [`column`] (the SHOW `createStatement` reads, the
/// batch tests' id columns), never a second closure spelling. The
/// variant discrimination itself is core's canonical
/// [`Value::as_string`](relate_core::Value::as_string) accessor,
/// the same one the graph layer's research kernels read through.
pub(crate) fn string_cell(value: &Value) -> Option<String> {
    value.as_string().map(str::to_string)
}

/// The list cell of a result column: `Some` for a [`Value::List`] cell,
/// `None` for any other variant — the list-column reader beside
/// [`string_cell`] and [`int_cell`], so no test spells the extraction
/// conditional inline.
pub(crate) fn list_cell(value: &Value) -> Option<Value> {
    match value {
        Value::List(_) => Some(value.clone()),
        _ => None,
    }
}

/// The integer cell of a value: `Some` for [`Value::Int`], `None` for any
/// other variant — the int-column reader beside [`string_cell`], so no
/// test spells the extraction conditional inline. Stricter than
/// [`Value::as_int`], which coerces whole-number floats: a result column
/// reads back exactly the type that was written, so a `Float` cell
/// reaching an int assertion should fail, not coerce silently.
pub(crate) fn int_cell(value: &Value) -> Option<i64> {
    match value {
        Value::Int(value) => Some(*value),
        _ => None,
    }
}

/// A write scratch reading at `ts`, its id allocators continuing the graph's.
pub(crate) fn write_scratch_at(g: &Graph, ts: u64) -> RefCell<WriteScratch> {
    RefCell::new(WriteScratch::new(
        ts,
        Arc::new(AtomicU64::new(g.next_node_id() as u64)),
        Arc::new(AtomicU64::new(g.next_rel_id() as u64)),
    ))
}

/// A write scratch reading at the graph's current commit.
pub(crate) fn write_scratch(g: &Graph) -> RefCell<WriteScratch> {
    write_scratch_at(g, g.commit_ts())
}

/// Run `cypher` on `scratch` through `execute`, constraint validation
/// included. Nothing reaches the graph until `commit_scratch`.
pub(crate) fn exec_write(
    g: &Graph,
    scratch: &RefCell<WriteScratch>,
    cypher: &str,
) -> Result<QueryResult> {
    run_write(g, scratch, cypher, execute)
}

/// [`exec_write`] through `execute_unvalidated`: the writes stay in the
/// slice the next constraint validation of `scratch` checks.
pub(crate) fn exec_write_unvalidated(
    g: &Graph,
    scratch: &RefCell<WriteScratch>,
    cypher: &str,
) -> Result<QueryResult> {
    run_write(g, scratch, cypher, execute_unvalidated)
}

pub(crate) use crate::test_support::{config_with_floor, ints, map};

/// One autocommit statement: `cypher` on a fresh scratch through `execute`,
/// committed when it succeeds and discarded when it fails. It composes this
/// module's write machinery, so it lives here rather than in the
/// forest-neutral `crate::test_support`.
pub(crate) fn exec_autocommit(g: &Graph, cypher: &str) -> Result<QueryResult> {
    let scratch = write_scratch(g);
    let result = exec_write(g, &scratch, cypher)?;
    commit_scratch(g, scratch);
    Ok(result)
}

/// [`exec_write`] on a fresh scratch, unwrapped; the scratch is dropped
/// without committing, so the graph is untouched. The uncommitted
/// counterpart of [`exec_autocommit`].
pub(crate) fn exec_write_uncommitted(g: &Graph, cypher: &str) -> QueryResult {
    exec_write(g, &write_scratch(g), cypher).unwrap()
}

fn run_write(
    g: &Graph,
    scratch: &RefCell<WriteScratch>,
    cypher: &str,
    run: fn(&ExecGraph<'_>, &ExecRequest<'_>) -> Result<QueryResult>,
) -> Result<QueryResult> {
    let p = plan(parse_and_lower(cypher).unwrap(), &mut |_| false).unwrap();
    run(
        &ExecGraph::write(g, scratch),
        &ExecRequest::new(&p, &BTreeMap::new(), cypher),
    )
}

/// Apply `scratch`'s redo to the graph as the next commit. A scratch that
/// wrote nothing commits nothing, as a read-only statement does.
pub(crate) fn commit_scratch(g: &Graph, scratch: RefCell<WriteScratch>) {
    let ops = scratch.into_inner().take_redo();
    if !ops.is_empty() {
        g.apply_redo(ops, g.commit_ts() + 1).unwrap();
    }
}

/// The ADR-0486 range-start fixture: 20 `P` nodes, `values.len()` `Q`
/// nodes each carrying `prop = values[i]`, and one `LINKS` rel from
/// `P[i % 20]` to `Q[i]`. One pattern over it —
/// `(p:P)-[:LINKS]->(q:Q) WHERE q.<prop> < <bound>` with a bound the
/// first few values satisfy — prices the `Q` side at its range window
/// and the `P` side at its degree-scaled label scan, which only the
/// ladder's range level separates: at label rows alone both sides scale
/// to the rel count (a tie that keeps the written `P` start), so the
/// start displaces onto `Q` exactly when the window is priced. Every
/// `Q` has exactly one rel, so each window candidate walks one hop.
pub(crate) fn range_start_graph(prop: &str, values: &[Value]) -> Graph {
    let g = Graph::default();
    g.begin_write().unwrap();
    let ps: Vec<_> = (0..20)
        .map(|_| g.create_node(vec!["P".into()], BTreeMap::new()).unwrap())
        .collect();
    for (i, value) in values.iter().enumerate() {
        let q = g
            .create_node(
                vec!["Q".into()],
                BTreeMap::from([(prop.to_string(), value.clone())]),
            )
            .unwrap();
        g.create_rel(ps[i % ps.len()], q, "LINKS".into(), BTreeMap::new())
            .unwrap();
    }
    g.commit_write();
    g
}

/// A one-argument aggregate call over a caller-chosen operand — the
/// parameterized form of the shape the aggregate tests, the charge tests,
/// the horizontal rewrite's tests, and the match-predicate test all build.
/// One definition, so the spelling cannot drift between them.
pub(crate) fn agg_fn_over(name: &str, arg: Expr) -> Expr {
    Expr::Function {
        name: name.into(),
        distinct: false,
        args: vec![arg],
    }
}

/// [`agg_fn_over`] over the shared `x` column.
pub(crate) fn agg_fn(name: &str) -> Expr {
    agg_fn_over(name, Expr::Variable("x".into()))
}

/// A group of one row per `x` binding (the aggregate tests' row fixture,
/// shared with the charge-test family's module). One definition, so the
/// two test modules cannot drift between spellings: the i64 convenience
/// delegates to [`group_with_vals`], the one row-building loop.
pub(crate) fn group_with_xs(xs: &[i64]) -> Vec<Row> {
    group_with_vals(&xs.iter().map(|&i| Value::Int(i)).collect::<Vec<_>>())
}

/// [`group_with_xs`] with caller-chosen values — the one row-building loop
/// both spellings read.
pub(crate) fn group_with_vals(vals: &[Value]) -> Vec<Row> {
    vals.iter()
        .map(|v| {
            let mut row = Row::new();
            row.insert("x".into(), v.clone());
            row
        })
        .collect()
}

/// The two-argument percentile call over the shared `x` column, with the
/// literal percentile `p`.
pub(crate) fn percentile_fn(name: &str, p: f64) -> Expr {
    Expr::Function {
        name: name.into(),
        distinct: false,
        args: vec![Expr::Variable("x".into()), Expr::Float(p)],
    }
}

/// A duration value for the aggregation test modules (one definition,
/// three call sites: aggregate_tests, pushdown_tests, match_clause).
pub(crate) fn duration(months: i64, days: i64, nanos: i128) -> Value {
    Value::Temporal(relate_core::temporal::Temporal::Duration {
        months,
        days,
        nanos,
    })
}

/// The node id of a `RETURN n` cell (one definition for every test that
/// reads a node back out of a result row).
pub(crate) fn node_id(value: &Value) -> NodeId {
    let Value::Node(n) = value else {
        panic!("expected a node, got {value:?}");
    };
    n.id
}

/// The relationship id of a `RETURN r` cell (the `node_id` twin, same one
/// definition for every test).
pub(crate) fn rel_id(value: &Value) -> RelId {
    let Value::Relationship(r) = value else {
        panic!("expected a relationship, got {value:?}");
    };
    r.id
}

/// The path value of a `RETURN p` cell — the `node_id`/`rel_id` family's
/// path member, in the shape its consumer reads: `None` on a non-path
/// cell, so `column`'s own panic (which names the column) stays the one
/// spelling for a wrong-variant cell; the twins keep their direct-cell
/// panics because their callers read cells by row, not by column.
pub(crate) fn path_value(value: &Value) -> Option<Arc<relate_core::Path>> {
    let Value::Path(p) = value else {
        return None;
    };
    Some(Arc::clone(p))
}

/// A built graph and its node and relationship ids, by index.
pub(crate) struct Built {
    pub graph: Graph,
    pub nodes: Vec<NodeId>,
    pub rels: Vec<RelId>,
}

/// Nodes `0..labels.len()` with `id: i` and the labels `labels[i]`, and
/// relationships `(from, to, type)` between node indexes with `k` = their
/// index.
pub(crate) fn build(labels: &[&[&str]], rels: &[(usize, usize, &str)]) -> Built {
    let graph = Graph::default();
    graph.begin_write().unwrap();
    let nodes: Vec<NodeId> = labels
        .iter()
        .enumerate()
        .map(|(i, labels)| {
            let labels = labels.iter().map(|l| l.to_string()).collect();
            let props = BTreeMap::from([("id".to_string(), Value::Int(i as i64))]);
            graph.create_node(labels, props).unwrap()
        })
        .collect();
    let rels = rels
        .iter()
        .enumerate()
        .map(|(k, &(a, b, t))| {
            let props = BTreeMap::from([("k".to_string(), Value::Int(k as i64))]);
            graph
                .create_rel(nodes[a], nodes[b], t.into(), props)
                .unwrap()
        })
        .collect();
    graph.commit_write();
    Built { graph, nodes, rels }
}

/// Builds a graph in one write, keyed by the names tests give its nodes.
pub(crate) struct Builder(Graph);

impl Builder {
    pub(crate) fn new() -> Self {
        let g = Graph::default();
        g.begin_write().unwrap();
        Self(g)
    }

    /// A node with `labels` and its name as `id`.
    pub(crate) fn node(&self, id: &str, labels: &[&str]) -> NodeId {
        let props = BTreeMap::from([("id".into(), Value::String(id.into()))]);
        self.create(labels, props)
    }

    /// A node with `labels`, its name as `id`, and extra properties.
    pub(crate) fn prop_node(&self, id: &str, labels: &[&str], props: &[(&str, Value)]) -> NodeId {
        let mut map = BTreeMap::from([("id".into(), Value::String(id.into()))]);
        for (key, value) in props {
            map.insert((*key).into(), value.clone());
        }
        self.create(labels, map)
    }

    /// A node with `labels` and no properties.
    pub(crate) fn anon(&self, labels: &[&str]) -> NodeId {
        self.create(labels, BTreeMap::new())
    }

    fn create(&self, labels: &[&str], props: BTreeMap<String, Value>) -> NodeId {
        let labels = labels.iter().map(|l| l.to_string()).collect();
        self.0.create_node(labels, props).unwrap()
    }

    /// A relationship of type `t` with `props`.
    pub(crate) fn rel(&self, from: NodeId, to: NodeId, t: &str, props: &[(&str, Value)]) {
        let props = props.iter().map(|(k, v)| (k.to_string(), v.clone()));
        self.0
            .create_rel(from, to, t.into(), props.collect())
            .unwrap();
    }

    /// An `E` relationship without properties.
    pub(crate) fn edge(&self, from: NodeId, to: NodeId) {
        self.rel(from, to, "E", &[]);
    }

    /// `E` relationships along `nodes`.
    pub(crate) fn chain(&self, nodes: &[NodeId]) {
        for pair in nodes.windows(2) {
            self.edge(pair[0], pair[1]);
        }
    }

    pub(crate) fn done(self) -> Graph {
        self.0.commit_write();
        self.0
    }
}

// The frontier family's shared test fixtures (stellar structure-4 hoist):
// the pattern-lowering and IR-constructor helpers every frontier suite
// reuses — the one canonical spelling of "lower this Cypher to patterns"
// and "build this IR shape" (`nfa_tests`' own assertions read them like
// every other suite).

use relate_ir::{
    Clause, Direction, NodePattern, PathPattern, PatternElement, QuantifiedPathPattern, RelPattern,
};

use super::kernels::frontier::{run, FrontierRun, WalkBudget};
use super::kernels::frontier_tables::fresh_tables;
use super::kernels::nfa::{compile, FrontierNfa, Pool};
use super::kernels::rearival::RuleBuffers;
use crate::pattern_engine::PatternEngine;

/// The lowered `MATCH` patterns of `cypher` — the front end's own
/// shapes, the coverage harness's route (no planner orientation):
/// these tests compile what lowering actually produces, the shapes the
/// scan-segment coverage report pins.
pub(crate) fn patterns(cypher: &str) -> Vec<PathPattern> {
    let query: Query = parse_and_lower(cypher).unwrap();
    query
        .parts
        .iter()
        .flat_map(|part| {
            part.clauses.iter().filter_map(|clause| match clause {
                Clause::Match { patterns, .. } => Some(patterns.as_slice()),
                _ => None,
            })
        })
        .flatten()
        .cloned()
        .collect()
}

/// A named unconstrained node — the IR-constructor vocabulary's base.
pub(crate) fn named_node(name: &str) -> NodePattern {
    NodePattern {
        variable: Some(name.into()),
        ..NodePattern::anonymous()
    }
}

/// A plain one-hop undirected rel over a static type list.
pub(crate) fn one_hop_rel(types: &[&str]) -> RelPattern {
    RelPattern {
        variable: None,
        types: types.iter().map(|name| (*name).into()).collect(),
        type_expr: None,
        direction: Direction::Both,
        properties: None,
        var_length: false,
        min_hops: 1,
        max_hops: Some(1),
        where_clause: None,
        quantified: false,
    }
}

/// A var-length rel as the front end lowers it inside a group: the
/// `*m..n` bounds on the rel element itself (`quantify_var_length`
/// runs top level only).
pub(crate) fn var_length_rel(types: &[&str], min: u32, max: Option<u32>) -> RelPattern {
    RelPattern {
        var_length: true,
        min_hops: min,
        max_hops: max,
        ..one_hop_rel(types)
    }
}

/// A quantified path part over `elements` with the given bounds.
pub(crate) fn quantified(
    elements: Vec<PatternElement>,
    min: u32,
    max: Option<u32>,
) -> QuantifiedPathPattern {
    QuantifiedPathPattern {
        subpath_variable: None,
        elements,
        where_clause: None,
        min,
        max,
    }
}

/// A path pattern over `elements` with the compact projections cleared
/// (lowering clears them when a `Quantified` element is present; the
/// compilers read the lossless `elements`).
pub(crate) fn path_pattern(elements: Vec<PatternElement>) -> PathPattern {
    PathPattern {
        variable: None,
        subpath_variable: None,
        elements,
        nodes: Vec::new(),
        rels: Vec::new(),
        where_clause: None,
        mode: None,
        selector: None,
        is_shortest_path_function: false,
    }
}

/// The endpoint nodes of `run`, sorted — the distinct-endpoint answer.
/// The frontier family's one spelling (the runner suite and the
/// re-arrival suite share it).
pub(crate) fn endpoint_nodes(run: &FrontierRun) -> Vec<NodeId> {
    let mut nodes: Vec<NodeId> = run.endpoints.iter().map(|e| e.node).collect();
    nodes.sort_unstable();
    nodes
}

/// Compile `cypher`'s first `MATCH` pattern against `graph`'s store on
/// the auto rung and run the frontier from `seed` over a read view, in
/// `pool`, under the standard budget with fresh tables and a served run —
/// the frontier suites' one run-harness spelling (the runner suite's
/// pins, the re-arrival suite's endpoint comparisons, and its
/// direct-rule pins' served runs): an authored panic on either failure,
/// the decline named. The low-level budget, recycling, and
/// overlay-visibility pins drive `kernels::frontier::run` directly
/// instead — this is the happy-path spelling, never the only one.
pub(crate) fn served_frontier_run(
    cypher: &str,
    graph: &Graph,
    seed: NodeId,
    pool: Pool,
) -> FrontierRun {
    let nfa = compile(graph, &patterns(cypher)[0], PatternEngine::Auto)
        .unwrap_or_else(|| panic!("{cypher} should compile"));
    served_nfa_run(&nfa, graph, seed, pool)
}

/// [`served_frontier_run`]'s run half over an already-compiled `nfa` —
/// the forced-rung suites compile with the rung they pin
/// (`PatternEngine::Nfa`) and serve the run through the same harness.
pub(crate) fn served_nfa_run(
    nfa: &FrontierNfa,
    graph: &Graph,
    seed: NodeId,
    pool: Pool,
) -> FrontierRun {
    let view = ExecGraph::read(graph);
    run(
        ExecCtx::new(&view, &BTreeMap::new(), None),
        seed,
        nfa,
        WalkBudget::standard(),
        pool,
        &mut fresh_tables(nfa),
        &mut RuleBuffers::new(),
    )
    .unwrap_or_else(|error| panic!("the frontier should run: {error}"))
    .unwrap_served()
}

/// The multiset row comparison (the kernels' own differential idiom):
/// `Value` carries no total order, the debug form is one, and equal
/// values print equal. The one spelling — the frontier rider suite and
/// the statement differential compare their answer sets through it.
pub(crate) fn same_rows(a: &QueryResult, b: &QueryResult) -> bool {
    if a.columns != b.columns {
        return false;
    }
    let mut a = a.rows.clone();
    let mut b = b.rows.clone();
    a.sort_by_key(|row| format!("{row:?}"));
    b.sort_by_key(|row| format!("{row:?}"));
    a == b
}

/// The one capacity-refusal assertion for the charge-pin family (the
/// configured cap is spelled once per test, then checked here — never a
/// second pattern literal beside the configure call). Returns the
/// refusal's reported `used` figure for the pins that bound its
/// overshoot. Shared by the aggregate-charge and projection test homes.
pub(crate) fn assert_expression_cap(error: &relate_core::Error, cap: u64) -> u64 {
    let relate_core::Error::Capacity {
        used,
        limit,
        kind: relate_core::CapacityKind::Expression,
        ..
    } = error
    else {
        panic!("the typed expression-capacity refusal at cap {cap}: {error}");
    };
    assert_eq!(*limit, cap, "{error}");
    *used
}

/// The one crossing-element bound the LABS-781 pins assert: the
/// refusal's reported figure is the element that crossed the cap —
/// overshoot bounded by that one element's own byte figure — never the
/// whole container the fold-time pre-charge used to report.
pub(crate) fn assert_crossing_element(used: u64, cap: u64, one_element: u64) {
    assert!(
        used > cap && used <= cap + one_element,
        "the refusal reports the crossing element ({used} over cap {cap}), \
not the whole container"
    );
}

/// The one group-landing bound the fold-served and keyed pins assert:
/// the workspace entry delta at `boundary` — the largest slot-growth
/// step the pin's group count can hit — carrying the pin's largest key
/// (`largest_key`), plus the one-element key-half vals buffer the
/// table retains beside the key.
pub(crate) fn group_landing_bound(boundary: usize, largest_key: &Value) -> u64 {
    crate::workspace::group_entry_insert_delta(
        boundary,
        &[crate::group_key::GroupKey::new(largest_key)],
    ) + crate::workspace::value_buffer_bytes(1)
}

/// [`assert_crossing_element`] for a landed row: one generous
/// `bindings`-wide row figure.
pub(crate) fn assert_crossing_row(used: u64, cap: u64, bindings: usize) {
    assert_crossing_element(
        used,
        cap,
        crate::workspace::retained_rows_bytes(1, bindings),
    );
}
