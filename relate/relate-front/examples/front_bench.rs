//! Parse-pipeline profiling harness for the Cypher front-end.
//!
//! A dev tool (not CI-gated): measures the wall-clock cost and the exact
//! heap-operation counts of the three front-end stages over a statement
//! corpus —
//!
//! - `lex`: `relate_parser::lexer::tokenize` (token stream only)
//! - `parse`: `relate_parser::parse` (full AST)
//! - `lower`: `relate_front::parse_and_lower` (AST lowered to
//!   planner IR)
//!
//! Heap counts come from a counting global allocator wrapping `System`;
//! every `alloc`/`dealloc`/`realloc` in the process during the timed loop
//! is tallied (per-statement figures are the timed loop's delta divided
//! by statements processed). Timing is a fixed-duration loop over the
//! corpus with `std::hint::black_box` sinks so nothing is optimized away.
//!
//! The corpus is `\x1e`-separated Cypher statements; regenerate it with
//! `python3 scripts/front_bench_corpus.py` (deduplicated statements from
//! `testing/relate-query-latency/workloads/**/*.cypher` plus, when
//! `TCK_CORPUS` points at the spec-harness checkout's `features/tck/`,
//! that corpus's `.feature` doc strings, written to
//! `target/front-bench-corpus.txt` by default).
//!
//! ```text
//! cargo build --release -p relate-front --example front_bench
//! python3 scripts/front_bench_corpus.py
//! target/release/examples/front_bench target/front-bench-corpus.txt lower 8
//! ```
//!
//! Usage: `front_bench <corpus.txt> <lex|parse|lower> [seconds]`
//! (seconds defaults to 10).

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// The timed front-end stage. The string↔variant vocabulary is spelled
/// once, in [`MODES`] below — [`Mode::from_arg`], [`Mode::name`] and the
/// usage / unknown-mode error list all consult that one table, so adding
/// or renaming a mode is a one-row edit.
#[derive(Copy, Clone, PartialEq)]
enum Mode {
    Lex,
    Parse,
    Lower,
}

/// The one mode vocabulary: the CLI spelling and the variant per row.
const MODES: &[(&str, Mode)] = &[
    ("lex", Mode::Lex),
    ("parse", Mode::Parse),
    ("lower", Mode::Lower),
];

impl Mode {
    fn from_arg(arg: &str) -> Option<Self> {
        MODES.iter().find(|(name, _)| *name == arg).map(|(_, m)| *m)
    }

    fn name(&self) -> &'static str {
        MODES
            .iter()
            .find(|(_, mode)| mode == self)
            .map(|(name, _)| *name)
            .expect("every Mode has a MODES row")
    }

    /// The mode spellings joined by `sep`, in table order — the usage
    /// line's `<lex|parse|lower>` and the unknown-mode error's list.
    fn spellings(sep: &str) -> String {
        MODES
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(sep)
    }

    /// Run `text` through this stage once, folding the outcome into `out`
    /// (a token-count sink for `lex`, ok/err counts for `parse`/`lower`).
    /// Both results and sinks are black-boxed so nothing is optimized
    /// away.
    fn run(&self, text: &str, out: &mut Outcome) {
        match self {
            Mode::Lex => {
                let tokens = relate_parser::lexer::tokenize(text);
                out.sink = out.sink.wrapping_add(tokens.len());
            }
            Mode::Parse => match relate_parser::parse(text) {
                Ok(stmt) => {
                    out.ok += 1;
                    black_box(&stmt);
                }
                Err(e) => {
                    out.err += 1;
                    black_box(&e);
                }
            },
            Mode::Lower => match relate_front::parse_and_lower(text) {
                Ok(q) => {
                    out.ok += 1;
                    black_box(&q);
                }
                Err(e) => {
                    out.err += 1;
                    black_box(&e);
                }
            },
        }
    }

    /// The report line's per-mode detail field: the mean token sink per
    /// statement for `lex`, the ok/err counts for `parse`/`lower`.
    fn detail(&self, m: &Measurements, statements: usize) -> String {
        match self {
            Mode::Lex => format!("sink={}", m.outcome.sink / m.passes / statements),
            Mode::Parse | Mode::Lower => {
                format!("ok={} err={}", m.outcome.ok, m.outcome.err)
            }
        }
    }
}

/// The parsed argv: corpus path, stage, timing budget. Built by
/// [`Config::from_args`], which reports usage errors as values (never
/// exiting inside the parse), so `main` owns every exit code and the
/// error path stays checkable.
struct Config {
    corpus_path: String,
    mode: Mode,
    budget_s: f64,
}

impl Config {
    /// Parse `args` (argv, including the program name). `Err` carries the
    /// complete usage message for `main` to print.
    fn from_args(args: &[String]) -> Result<Self, String> {
        let program = args.first().map(String::as_str).unwrap_or("front_bench");
        if args.len() < 3 {
            return Err(format!(
                "usage: {program} <corpus.txt> <{}> [seconds]",
                Mode::spellings("|")
            ));
        }
        let mode = Mode::from_arg(&args[2]).ok_or_else(|| {
            format!(
                "unknown mode {:?}: expected one of {}",
                args[2],
                Mode::spellings(", ")
            )
        })?;
        let budget_s = match args.get(3) {
            Some(s) => s
                .parse()
                .map_err(|_| format!("seconds must be a number, got {s:?}"))?,
            None => 10.0,
        };
        Ok(Self {
            corpus_path: args[1].clone(),
            mode,
            budget_s,
        })
    }
}

/// One staged outcome counter, folded by [`Mode::run`] across the whole
/// timed loop.
struct Outcome {
    sink: usize,
    ok: usize,
    err: usize,
}

/// The timed loop's results: the pass count, the wall-clock seconds, the
/// heap-operation deltas (baselined after corpus loading, so only the
/// loop's allocations are counted) and the outcome counters — everything
/// [`report`] prints.
struct Measurements {
    passes: usize,
    secs: f64,
    allocs: usize,
    alloc_bytes: usize,
    frees: usize,
    reallocs: usize,
    outcome: Outcome,
}

// ---- counting allocator: exact heap-op counts ----

struct CountingAlloc;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);

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
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static A: CountingAlloc = CountingAlloc;

/// Read the corpus file — the one reader, so the read failure and its
/// message exist once. The caller splits the raw text on `\x1e` (the
/// corpus format front_bench consumes) and borrows the statements from it.
fn read_corpus(config: &Config) -> Result<String, String> {
    std::fs::read_to_string(&config.corpus_path)
        .map_err(|e| format!("cannot read corpus {:?}: {e}", config.corpus_path))
}

/// Run the timed loop: fixed-duration passes over every statement of the
/// corpus, with the heap counters' baseline captured after corpus loading.
/// `None` when not even one full pass completed within the budget.
fn measure(config: &Config, corpus: &[&str]) -> Option<Measurements> {
    let baseline_allocs = ALLOCS.load(Ordering::Relaxed);
    let baseline_bytes = ALLOC_BYTES.load(Ordering::Relaxed);
    let baseline_frees = FREES.load(Ordering::Relaxed);
    let baseline_reallocs = REALLOCS.load(Ordering::Relaxed);
    let start = Instant::now();
    let mut passes = 0usize;
    let mut outcome = Outcome {
        sink: 0,
        ok: 0,
        err: 0,
    };
    while start.elapsed().as_secs_f64() < config.budget_s {
        for text in corpus {
            config.mode.run(text, &mut outcome);
        }
        passes += 1;
    }
    let secs = start.elapsed().as_secs_f64();
    if passes == 0 {
        return None;
    }
    Some(Measurements {
        passes,
        secs,
        allocs: ALLOCS.load(Ordering::Relaxed) - baseline_allocs,
        alloc_bytes: ALLOC_BYTES.load(Ordering::Relaxed) - baseline_bytes,
        frees: FREES.load(Ordering::Relaxed) - baseline_frees,
        reallocs: REALLOCS.load(Ordering::Relaxed) - baseline_reallocs,
        outcome,
    })
}

/// Print the report: the heap-operation counts per statement, then the
/// throughput line with its per-mode detail field.
fn report(config: &Config, corpus: &[&str], m: &Measurements) {
    let units = m.passes * corpus.len();
    println!(
        "heap: {} allocs ({:.1}/stmt, {} KiB), {} frees, {} reallocs",
        m.allocs,
        m.allocs as f64 / units as f64,
        m.alloc_bytes / 1024,
        m.frees,
        m.reallocs
    );

    let total_bytes: usize = corpus.iter().map(|s| s.len()).sum();
    let ms_per_pass = m.secs * 1000.0 / m.passes as f64;
    let mb_per_s = total_bytes as f64 * m.passes as f64 / m.secs / 1e6;
    let detail = config.mode.detail(m, corpus.len());
    println!(
        "{:<6} {:>7.3}s {} passes = {:.2} ms/pass, {:.1} MB/s, {detail}",
        config.mode.name(),
        m.secs,
        m.passes,
        ms_per_pass,
        mb_per_s,
    );
    black_box(m.outcome.sink);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config = Config::from_args(&args).unwrap_or_else(|usage| {
        eprintln!("{usage}");
        std::process::exit(2);
    });
    let raw = read_corpus(&config).unwrap_or_else(|message| {
        eprintln!("{message}");
        std::process::exit(1);
    });
    let corpus: Vec<&str> = raw.split('\x1e').collect();
    eprintln!(
        "corpus: {} statements, {} bytes",
        corpus.len(),
        corpus.iter().map(|s| s.len()).sum::<usize>()
    );
    let Some(measurements) = measure(&config, &corpus) else {
        eprintln!(
            "no full pass completed in {}s; raise the budget",
            config.budget_s
        );
        std::process::exit(1);
    };
    report(&config, &corpus, &measurements);
}
