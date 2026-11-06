//! PLAN-phase profiling harness for the query engine.
//!
//! A dev tool (not CI-gated): loops `EXPLAIN` over every `.cypher` query in
//! a directory against a prebuilt, read-only-opened store and reports the
//! per-statement phase split from the always-on phase timing (ADR-0491):
//!
//! - `parse`: front-end parse and lowering
//! - `plan`: the PLAN phase proper — binding, logical planning, admission
//!   classification
//! - `execute`: in EXPLAIN mode this is the plan render, not data access
//!
//! plus exact heap-operation counts (allocs/frees/reallocs per statement)
//! from a counting global allocator wrapping `System` — every heap op in
//! the process during the timed loop is tallied, so the per-statement
//! figures cover the whole statement path including dispatch. Timing is a
//! fixed-duration loop over the corpus with a warm-up pass and
//! `std::hint::black_box` sinks so nothing is optimized away.
//!
//! The store is a build artifact of the query-latency suite (ADR-0243),
//! never committed: produce the GBL SF0.1 store with
//! `make query-latency` (or `cargo run --release -p
//! relate-query-latency -- run --min-time 5`), which writes
//! `target/query-latency/graph-benchmark-ldbc-sf0.1.cl`; the matching
//! query corpus is
//! `testing/relate-query-latency/workloads/graph-benchmark-ldbc/queries`.
//!
//! ```text
//! cargo build --release -p relate-query-latency --bin plan_bench
//! target/release/plan_bench \
//!   target/query-latency/graph-benchmark-ldbc-sf0.1.cl \
//!   testing/relate-query-latency/workloads/graph-benchmark-ldbc/queries 15
//! ```
//!
//! Usage: `plan_bench <store.cl> <queries-dir> [seconds]`
//! (seconds defaults to 15).

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use clap::Parser;
use relate::{Database, OpenMode, OpenOptions, Result};

// ---- counting allocator: exact heap-op counts ----

struct CountingAlloc;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method only tallies a relaxed atomic beside the identical
// `System` call — the forwarded layout/pointer arguments reach `System`
// unchanged, so allocation correctness is `System`'s own.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(l.size(), Ordering::Relaxed);
        System.alloc(l)
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        FREES.fetch_add(1, Ordering::Relaxed);
        System.dealloc(p, l)
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Ordering::Relaxed);
        // `Vec`/`String` growth goes through here, so the byte tally
        // counts the requested size of every growth path too — the
        // figure is bytes requested, not live heap.
        ALLOC_BYTES.fetch_add(n, Ordering::Relaxed);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static A: CountingAlloc = CountingAlloc;

/// Loads every `.cypher` file in `dir` (sorted by path for a stable
/// corpus order), strips `//` comment lines, and wraps each statement in
/// `EXPLAIN` so the loop exercises parse + plan + render, never execution.
/// The failure policy is `Err(String)` — `main` owns the one exit site.
fn load_explain_queries(dir: &Path) -> std::result::Result<Vec<String>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read queries dir {dir:?}: {e}"))?;
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read entry in queries dir {dir:?}: {e}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "cypher") {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err(format!("no .cypher queries found in {dir:?}"));
    }
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let text =
                std::fs::read_to_string(p).map_err(|e| format!("cannot read query {p:?}: {e}"))?;
            let code: String = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join(" ");
            Ok(format!("EXPLAIN {}", code.trim().trim_end_matches(';')))
        })
        .collect()
}

#[derive(Parser, Debug)]
#[command(
    name = "plan_bench",
    about = "PLAN-phase micro-harness: EXPLAIN loop over a query corpus \
             (ADR-0508's measurement loop; dev tool, not CI-gated)"
)]
struct Args {
    /// Prebuilt, read-only-opened store path — the query-latency
    /// suite's build artifact (`make query-latency` writes it).
    store: String,
    /// Directory of `.cypher` query files to loop over.
    queries_dir: PathBuf,
    /// Timed-loop budget in seconds, after one warm-up pass.
    #[arg(default_value_t = 15.0)]
    seconds: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    // The one CLI-policy exit site: load failures render and exit here,
    // everything after this point reports through `Result`.
    let queries = load_explain_queries(&args.queries_dir).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    eprintln!("queries: {}", queries.len());
    let budget_s = args.seconds;

    let opts = OpenOptions {
        mode: OpenMode::ReadOnly,
        ..Default::default()
    };
    let db = Database::open_with(&args.store, opts)?;
    let mut conn = db.connect()?;

    // warm-up pass: the first plan per query primes whatever's primable
    for q in &queries {
        black_box(conn.query(q)?);
    }

    let baseline_allocs = ALLOCS.load(Ordering::Relaxed);
    let baseline_bytes = ALLOC_BYTES.load(Ordering::Relaxed);
    let baseline_frees = FREES.load(Ordering::Relaxed);
    let baseline_reallocs = REALLOCS.load(Ordering::Relaxed);
    let start = Instant::now();
    let mut passes = 0usize;
    let mut parse_ms = 0.0f64;
    let mut plan_ms = 0.0f64;
    let mut execute_ms = 0.0f64;
    while start.elapsed().as_secs_f64() < budget_s {
        for q in &queries {
            let r = conn.query(q)?;
            parse_ms += r.parse_ms();
            plan_ms += r.plan_ms();
            execute_ms += r.execute_ms();
            black_box(&r);
        }
        passes += 1;
    }
    let secs = start.elapsed().as_secs_f64();
    if passes == 0 {
        eprintln!("no full pass completed in {budget_s}s; raise the budget");
        std::process::exit(1);
    }

    let stmts = passes * queries.len();
    let allocs = ALLOCS.load(Ordering::Relaxed) - baseline_allocs;
    let alloc_bytes = ALLOC_BYTES.load(Ordering::Relaxed) - baseline_bytes;
    let frees = FREES.load(Ordering::Relaxed) - baseline_frees;
    let reallocs = REALLOCS.load(Ordering::Relaxed) - baseline_reallocs;
    println!(
        "explain-loop: {secs:.2}s, {passes} passes over {} queries = {stmts} statements",
        queries.len()
    );
    println!(
        "avg per statement: parse {:.1}us  plan {:.1}us  execute {:.1}us",
        parse_ms * 1000.0 / stmts as f64,
        plan_ms * 1000.0 / stmts as f64,
        execute_ms * 1000.0 / stmts as f64,
    );
    println!(
        "heap: {} allocs ({:.1}/stmt, {} KiB requested), {} frees ({:.1}/stmt), {} reallocs",
        allocs,
        allocs as f64 / stmts as f64,
        alloc_bytes / 1024,
        frees,
        frees as f64 / stmts as f64,
        reallocs,
    );
    Ok(())
}
