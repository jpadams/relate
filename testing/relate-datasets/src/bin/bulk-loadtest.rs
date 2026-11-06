//! Bulk-load load test: `create` / `merge-with-constraint` / `update` /
//! `delete`, one statement and one commit per batch, across a ladder of batch
//! sizes. See [`relate_datasets::bulk`] for what is measured and why the
//! `Person.id` uniqueness constraint is in place for every op except `create`.
//!
//! Runs single-threaded by design (determinism); use `fraud-loadtest
//! --workers` for the concurrent group-commit shape.

use std::path::PathBuf;

use clap::Parser;
use relate_datasets::bulk::{parse_ops, parse_sizes, run, BulkConfig, BulkOp, DEFAULT_SIZES};
use relate_datasets::loadtest::parse_cli_size;
use relate_datasets::memory::format_bytes;

#[derive(Parser, Debug)]
#[command(
    name = "bulk-loadtest",
    about = "Bulk create / merge / update / delete load test at 5k..100k rows per statement"
)]
struct Args {
    /// Operations to measure, comma-separated (`create`, `merge-with-constraint`, `update`, `delete`)
    #[arg(long, value_name = "LIST")]
    ops: Option<String>,
    /// Batch sizes, comma-separated (`5000`, `50_000`, `50k`)
    #[arg(long, value_name = "LIST")]
    sizes: Option<String>,
    /// Measured repetitions per (operation, size); each rep re-creates its base data, untimed
    #[arg(long, default_value_t = 1)]
    reps: usize,
    /// Unmeasured repetitions run first (each still gets its untimed setup)
    #[arg(long, default_value_t = 0)]
    warmup: usize,
    /// Directory for the per-measurement databases, or `:memory:` for a fresh in-memory database per measurement
    #[arg(long, default_value = "target/bulk-loadtest-data")]
    dir: String,
    /// Live graph + index cap: integer bytes, or a size with mb/mib/gb/gib (RELATE_LOGICAL_GRAPH_SIZE_LIMIT; 0 or omit is unlimited)
    #[arg(long, value_name = "SIZE", value_parser = parse_cli_size)]
    logical_graph_size_limit: Option<u64>,
    /// Report JSON path
    #[arg(long, default_value = "target/bulk-loadtest.json")]
    out: PathBuf,
}

/// Bad `--ops` / `--sizes` element: message on stderr, usage exit code.
fn usage_error(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

fn main() {
    let args = Args::parse();
    let ops = match &args.ops {
        Some(list) => parse_ops(list).unwrap_or_else(|e| usage_error(&e)),
        None => BulkOp::all(),
    };
    let sizes = match &args.sizes {
        Some(list) => parse_sizes(list).unwrap_or_else(|e| usage_error(&e)),
        None => DEFAULT_SIZES.to_vec(),
    };
    let cfg = BulkConfig {
        dir: args.dir.clone(),
        sizes,
        ops,
        reps: args.reps,
        warmup: args.warmup,
        out: Some(args.out.clone()),
        logical_graph_size_limit: args.logical_graph_size_limit,
    };
    println!(
        "bulk-loadtest: {} ops x {} sizes x {} rep(s) into {}",
        cfg.ops.len(),
        cfg.sizes.len(),
        cfg.reps,
        cfg.dir
    );
    match run(&cfg) {
        // `run()` writes `--out` itself (via `cfg.out`).
        Ok(report) => {
            println!(
                "\n{} measured rows in {} ms",
                report.total_rows(),
                report.duration_ms
            );
            println!(
                "{:<21} {:>3} {:>8} {:>8} {:>12} {:>9} {:>9} {:>9}",
                "op", "con", "rows", "ms", "rows/s", "p50", "p95", "p99"
            );
            for e in &report.results {
                let (p50, p95, p99) = match &e.latency_ns {
                    Some(l) => (
                        format!("{:.1}ms", l.p50 as f64 / 1e6),
                        format!("{:.1}ms", l.p95 as f64 / 1e6),
                        format!("{:.1}ms", l.p99 as f64 / 1e6),
                    ),
                    None => ("-".into(), "-".into(), "-".into()),
                };
                println!(
                    "{:<21} {:>3} {:>8} {:>8} {:>12.1} {:>9} {:>9} {:>9}{}",
                    e.op.id(),
                    if e.constraint { "yes" } else { "no" },
                    e.size,
                    e.duration_ms,
                    e.rows_per_sec,
                    p50,
                    p95,
                    p99,
                    match (&e.error, e.errors) {
                        (Some(msg), n) => format!("  errors={n}: {msg}"),
                        (None, 0) => String::new(),
                        (None, n) => format!("  errors={n}"),
                    }
                );
            }
            println!("(con = Person.id uniqueness constraint present during the op)");
            println!(
                "wrote {}  (peak RSS {})",
                args.out.display(),
                format_bytes(
                    report
                        .results
                        .iter()
                        .map(|e| e.rss_bytes)
                        .max()
                        .unwrap_or(0)
                )
            );
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
