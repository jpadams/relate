//! SmallName corpus benchmark (ADR-0509's spike measurement, promoted with
//! the type into `relate-names`).
//!
//! A dev tool (not CI-gated) that replays the parse corpus's name stream
//! through the per-statement symbol-table strategies defined in
//! `tests/tables/strategies.rs` (path-included below, so the strategies
//! have exactly one copy):
//!
//! - `string_map`        — baseline: std (SipHash) `HashMap<String, u32>`
//! - `string_fx`         — control: `FxHashMap<String, u32>` — the hasher
//!   swap *without* the representation change, to split the two effects
//! - `smallname_vec`     — flat `Vec<(SmallName, u32)>`, linear scan,
//!   names re-classified per lookup (raw-text replay)
//! - `smallname_fx`      — `FxHashMap<SmallName, u32>`, raw-text replay
//! - `smallname_vec_pre` — the Vec table on pre-classified keys (the
//!   wired-engine shape: the token already carries its `SmallName`, so the
//!   replay pays no per-lookup classification)
//! - `smallname_fx_pre`  — the Fx table on pre-classified keys
//!
//! The corpus is `\x1e`-separated Cypher statements. Per statement the
//! bench extracts every name occurrence in positional order — plain
//! identifiers per the measurement regex `[A-Za-z_][A-Za-z0-9_]*`, plus
//! backtick-quoted names (dequoted, so the Spilled arm's quoted-only
//! shapes exercise) — then runs intern+lookup per statement with a fresh
//! table, the way a per-statement scope table lives and dies.
//!
//! Heap counts come from a counting global allocator wrapping `System`
//! (the idiom copied from the parse-pipeline-perf `front_bench` harness):
//! one dedicated measured pass per strategy yields allocs/stmt (reallocs
//! tracked separately — `Vec` growth is `realloc` traffic, and without
//! the override the allocator trait's realloc default silently re-bills
//! it as fresh allocs), then a fixed-duration timed loop yields ns per
//! lookup (median over per-pass times).
//!
//! ```text
//! cargo build --release -p relate-names --example smallname_bench
//! target/release/examples/smallname_bench \
//!   /tmp/cypher-parse-profile/data/corpus.txt 5 smallname_fx_pre
//! ```
//!
//! Usage: `smallname_bench <corpus.txt> [seconds] [strategy]` (seconds
//! defaults to 5; strategy defaults to `all`; naming one strategy runs it
//! alone, avoiding the in-process order bias of a shared warm state).
//!
//! # Findings (2026-11-04, corpus `/tmp/cypher-parse-profile/data/corpus.txt`)
//!
//! Isolated-process runs (one strategy per process, per-pass median),
//! stable across reversed-order rounds:
//!
//! ```text
//! strategy            allocs/stmt        bytes/stmt   ns/lookup
//! string_map          17.16              1494.9       ~39
//! string_fx           17.16              1494.9       ~23
//! smallname_vec        4.08 (+1.58 rlc)    226.3       ~43
//! smallname_fx         5.90              1519.4       ~20
//! smallname_vec_pre    1.00 (+1.58 rlc)    128.0       ~34
//! smallname_fx_pre     2.81              1421.2       ~13
//! ```
//!
//! - Corpus replay: 53,781 name occurrences across 2,103 statements,
//!   3,873 distinct, 13 quoted; inline fit 87.9% of occurrences; distinct
//!   names per statement median 8 / p90 35 — matching the 2026-11-04
//!   measurement context
//! - **The cheap half of the lookup win is the hasher**: the `string_fx`
//!   control (owned-String keys on FxHash) halves ns/lookup (39 → 23)
//!   with zero representation churn
//! - **In the wired shape the representation adds a further real win**:
//!   pre-classified `smallname_fx_pre` runs ~13 ns/lookup — 1.7x faster
//!   than the `string_fx` control (hashing a bare `u64` beats hashing
//!   string bytes), 2.9x faster than the baseline — and 6x fewer
//!   allocations per statement (2.8 vs 17.2). The win exists only if
//!   tokens carry their `SmallName` (classification amortized at parse
//!   time); re-classifying per lookup (`smallname_fx`, ~20 ns) gives it
//!   all back to the control
//! - **The linear-scan Vec table is a no-go as the lookup structure**:
//!   even pre-classified (~34 ns) it loses to any Fx map; the p90-35
//!   statements scan too much. It is the memory-shape play only —
//!   128 bytes/stmt vs ~1500, 11.7x less allocated bytes than every
//!   hash-table variant (hashbrown growth churn dominates bytes for all
//!   map strategies; entry size is the same 32 bytes)
//! - `size_of::<SmallName>() == size_of::<String>() == 24` — the win is
//!   allocations and lookup cost, not struct width (asserted in the
//!   crate's tests)

// The strategies module is shared verbatim with tests/tables.rs (the one
// copy of the table shapes); the bench drives every strategy and both
// lookup arms, so no dead-code allow is needed on this inclusion.
#[path = "../tests/tables/strategies.rs"]
mod strategies;

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashSet;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use relate_names::{is_identifier_continue, is_identifier_start, SmallName};
use strategies::{SmallNameFxTable, SmallNameVecTable, StringFxTable, StringMapTable};

// ---- counting allocator: exact heap-op counts (front_bench idiom) ----

struct CountingAlloc;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(l.size(), Ordering::Relaxed);
        System.alloc(l)
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Ordering::Relaxed);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static A: CountingAlloc = CountingAlloc;

// ---- name-stream extraction ----

/// Every name occurrence in the statement, positional order: plain
/// identifiers (`[A-Za-z_][A-Za-z0-9_]*`, same semantics as the
/// measurement regex) plus backtick-quoted names dequoted — the tuple's
/// `usize` counts the quoted ones (they exercise the Spilled arm's
/// quoted-only shapes). Unterminated backticks contribute nothing (the
/// corpus statements are well-formed).
fn extract_names(statement: &str) -> (Vec<&str>, usize) {
    let bytes = statement.as_bytes();
    let mut names = Vec::new();
    let mut quoted = 0;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'`' {
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b'`' {
                    // `` inside a quoted name is an escaped backtick
                    if bytes.get(j + 1) == Some(&b'`') {
                        j += 2;
                        continue;
                    }
                    names.push(&statement[i + 1..j]);
                    quoted += 1;
                    i = j + 1;
                    break;
                }
                j += 1;
            }
            if j >= bytes.len() {
                i = j; // unterminated: nothing to contribute
            }
        } else if is_identifier_start(byte) {
            let mut j = i + 1;
            while j < bytes.len() && is_identifier_continue(bytes[j]) {
                j += 1;
            }
            names.push(&statement[i..j]);
            i = j;
        } else {
            i += 1;
        }
    }
    (names, quoted)
}

// ---- measurement ----

/// One strategy end to end behind one skeleton: a counted pass for
/// allocs/stmt, a warmup pass, then a fixed-duration timed loop for ns
/// per lookup (a lookup = one `step` call, the replay's only
/// per-occurrence work; median over per-pass times so one throttled pass
/// cannot drag the figure).
fn measure<T, I, F>(
    label: &str,
    per_statement: &[Vec<I>],
    budget_s: f64,
    new_table: fn() -> T,
    step: F,
) where
    F: Fn(&mut T, &I) -> u32,
{
    let statements = per_statement.len();
    let occurrences: usize = per_statement.iter().map(Vec::len).sum();

    // counted pass: fresh table per statement, snapshot deltas around it
    let a0 = ALLOCS.load(Ordering::Relaxed);
    let ab0 = ALLOC_BYTES.load(Ordering::Relaxed);
    let r0 = REALLOCS.load(Ordering::Relaxed);
    let mut sink = 0u64;
    for items in per_statement {
        let mut table = new_table();
        for item in items {
            sink = sink.wrapping_add(u64::from(step(&mut table, item)));
        }
    }
    let allocs = ALLOCS.load(Ordering::Relaxed) - a0;
    let alloc_bytes = ALLOC_BYTES.load(Ordering::Relaxed) - ab0;
    let reallocs = REALLOCS.load(Ordering::Relaxed) - r0;

    // warmup, then timed loop: per-pass durations, median taken (the
    // loop's wall clock only bounds the budget)
    for items in per_statement {
        let mut table = new_table();
        for item in items {
            sink = sink.wrapping_add(u64::from(step(&mut table, item)));
        }
    }
    let start = Instant::now();
    let mut pass_ns: Vec<f64> = Vec::new();
    while start.elapsed().as_secs_f64() < budget_s {
        let pass_start = Instant::now();
        for items in per_statement {
            let mut table = new_table();
            for item in items {
                sink = sink.wrapping_add(u64::from(step(&mut table, item)));
            }
        }
        pass_ns.push(pass_start.elapsed().as_nanos() as f64);
    }
    if pass_ns.is_empty() {
        eprintln!("{label}: no full pass completed in {budget_s}s; raise the budget");
        black_box(sink);
        return;
    }
    pass_ns.sort_by(|a, b| a.total_cmp(b));
    let median_pass_ns = pass_ns[pass_ns.len() / 2];
    let ns_per_lookup = median_pass_ns / occurrences as f64;
    println!(
        "{label:<18} allocs/stmt {:>7.2} (+reallocs {:>5.2})  bytes/stmt {:>9.1}  ns/lookup {:>8.1}  passes {}",
        allocs as f64 / statements as f64,
        reallocs as f64 / statements as f64,
        alloc_bytes as f64 / statements as f64,
        ns_per_lookup,
        pass_ns.len(),
    );
    black_box(sink);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: {} <corpus.txt> [seconds] \
             [string_map|string_fx|smallname_vec|smallname_fx|smallname_vec_pre|smallname_fx_pre|all]",
            args.first().map(String::as_str).unwrap_or("smallname_bench")
        );
        std::process::exit(2);
    }
    let corpus_path = &args[1];
    let budget_s: f64 = args
        .get(2)
        .map(|s| {
            s.parse().unwrap_or_else(|_| {
                eprintln!("seconds must be a number, got {s:?}");
                std::process::exit(2);
            })
        })
        .unwrap_or(5.0);
    const STRATEGIES: &[&str] = &[
        "string_map",
        "string_fx",
        "smallname_vec",
        "smallname_fx",
        "smallname_vec_pre",
        "smallname_fx_pre",
        "all",
    ];
    let strategy = args.get(3).map(String::as_str).unwrap_or("all");
    if !STRATEGIES.contains(&strategy) {
        eprintln!(
            "unknown strategy {strategy:?}: expected one of {}",
            STRATEGIES.join(", ")
        );
        std::process::exit(2);
    }

    let raw = std::fs::read_to_string(corpus_path).unwrap_or_else(|e| {
        eprintln!("cannot read corpus {corpus_path:?}: {e}");
        std::process::exit(1);
    });
    let corpus: Vec<&str> = raw.split('\x1e').collect();
    eprintln!("corpus: {} statements, {} bytes", corpus.len(), raw.len());

    // extraction is preprocessing: the timed loops measure intern+lookup
    let (per_statement, quoted_per_stmt): (Vec<Vec<&str>>, Vec<usize>) =
        corpus.iter().map(|s| extract_names(s)).unzip();
    let occurrences: usize = per_statement.iter().map(Vec::len).sum();
    let quoted: usize = quoted_per_stmt.iter().sum();
    let inline: usize = per_statement
        .iter()
        .flat_map(Vec::as_slice)
        .filter(|n| matches!(SmallName::from_name(n), SmallName::Inline(_)))
        .count();
    let distinct: HashSet<&str> = per_statement
        .iter()
        .flat_map(Vec::as_slice)
        .copied()
        .collect();
    let mut distinct_per_stmt: Vec<usize> = per_statement
        .iter()
        .map(|names| names.iter().collect::<HashSet<_>>().len())
        .collect();
    distinct_per_stmt.sort_unstable();
    if distinct_per_stmt.is_empty() || occurrences == 0 {
        eprintln!("corpus has no name occurrences; nothing to measure");
        std::process::exit(1);
    }

    let median = distinct_per_stmt[distinct_per_stmt.len() / 2];
    let p90 = distinct_per_stmt[(distinct_per_stmt.len() * 90) / 100];
    eprintln!(
        "names: {} occurrences, {} distinct, {} quoted, inline fit {:.1}%, distinct/stmt median {} p90 {}",
        occurrences,
        distinct.len(),
        quoted,
        100.0 * inline as f64 / occurrences as f64,
        median,
        p90,
    );

    // the wired-shape stream: names classified once, up front
    let per_statement_small: Vec<Vec<SmallName>> = per_statement
        .iter()
        .map(|names| names.iter().map(|n| SmallName::from_name(n)).collect())
        .collect();

    if strategy == "all" || strategy == "string_map" {
        measure(
            "string_map",
            &per_statement,
            budget_s,
            StringMapTable::new,
            |t: &mut StringMapTable, n: &&str| StringMapTable::lookup_or_insert(t, n),
        );
    }
    if strategy == "all" || strategy == "string_fx" {
        measure(
            "string_fx",
            &per_statement,
            budget_s,
            StringFxTable::new,
            |t: &mut StringFxTable, n: &&str| StringFxTable::lookup_or_insert(t, n),
        );
    }
    if strategy == "all" || strategy == "smallname_vec" {
        measure(
            "smallname_vec",
            &per_statement,
            budget_s,
            SmallNameVecTable::new,
            |t: &mut SmallNameVecTable, n: &&str| SmallNameVecTable::lookup_or_insert(t, n),
        );
    }
    if strategy == "all" || strategy == "smallname_fx" {
        measure(
            "smallname_fx",
            &per_statement,
            budget_s,
            SmallNameFxTable::new,
            |t: &mut SmallNameFxTable, n: &&str| SmallNameFxTable::lookup_or_insert(t, n),
        );
    }
    if strategy == "all" || strategy == "smallname_vec_pre" {
        measure(
            "smallname_vec_pre",
            &per_statement_small,
            budget_s,
            SmallNameVecTable::new,
            |t: &mut SmallNameVecTable, k: &SmallName| {
                SmallNameVecTable::lookup_or_insert_key(t, k.clone())
            },
        );
    }
    if strategy == "all" || strategy == "smallname_fx_pre" {
        measure(
            "smallname_fx_pre",
            &per_statement_small,
            budget_s,
            SmallNameFxTable::new,
            |t: &mut SmallNameFxTable, k: &SmallName| {
                SmallNameFxTable::lookup_or_insert_key(t, k.clone())
            },
        );
    }
}
