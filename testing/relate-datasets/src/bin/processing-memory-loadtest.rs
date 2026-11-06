use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use relate_datasets::harness::default_bolt_bin;
use relate_datasets::processing_harness::{default_store, run, CaseReport, Config};
use relate_datasets::processing_workload::{
    parse_index_mode, parse_ratio, parse_read_query, Controls, IndexMode, ReadQuery, WorkloadReport,
};

#[derive(Parser, Debug)]
#[command(
    name = "processing-memory-loadtest",
    about = "Push Relate processing-memory controls and record RSS"
)]
struct Args {
    /// Directory for this run. Each case uses a subdirectory.
    #[arg(long)]
    uri: Option<PathBuf>,
    /// Path to the relate CLI (`serve`). Default: sibling of this
    /// executable, then target/release/relate.
    #[arg(long)]
    serve_bin: Option<PathBuf>,
    /// Report JSON path
    #[arg(long, default_value = "target/processing-memory-loadtest.json")]
    out: PathBuf,
    /// Multiplier for ledger width. Depth stays at 8 until scale 5.
    #[arg(long, default_value_t = 1)]
    graph_scale: u64,
    /// Concurrent Bolt clients. Each keeps one connection.
    #[arg(long, default_value_t = 4)]
    workers: u64,
    /// Read and write parts, for example 80:20.
    #[arg(long, default_value = "80:20")]
    read_write_ratio: String,
    /// Shared connection cap. 0 is unlimited.
    #[arg(long, default_value_t = 0)]
    max_connections: u64,
    /// Concurrent logical workspace bytes. 0 leaves admission off.
    #[arg(long, default_value_t = 0)]
    concurrent_logical_workspace_limit: u64,
    /// Measured operations after warmup.
    #[arg(long, default_value_t = 40)]
    ops: u64,
    /// Operations run before measurement. Not counted.
    #[arg(long, default_value_t = 8)]
    warmup_ops: u64,
    /// Seed for the operation shuffle.
    #[arg(long, default_value_t = 1)]
    workload_seed: u64,
    /// indexed, scan, or both.
    #[arg(long, default_value = "both")]
    index_mode: String,
    /// mix, deep-paths, or size-sort. mix rotates the six read statements.
    #[arg(long, default_value = "mix")]
    read_query: String,
    /// Run the configured workload only.
    #[arg(long, default_value_t = false)]
    skip_probes: bool,
    /// One case: connections, unlimited-connections, concurrent-logical-workspace-limit,
    /// peak-token-rss, or workload.
    #[arg(long)]
    probe: Option<String>,
    /// Repetitions for each logical-byte calibration shape.
    #[arg(long, default_value_t = 3)]
    calibration_repeats: u32,
    /// Milliseconds to hold each calibrated workspace at its live peak.
    #[arg(long, default_value_t = 2500)]
    calibration_hold_ms: u64,
    /// Restrict logical-byte calibration to one workspace family.
    #[arg(long)]
    calibration_family: Option<String>,
}

/// 1024-based, matching the memory-shape loadtest. GB once the value
/// reaches 1024 MB.
fn format_mem(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{mb:.2} MB")
    }
}

fn print_case(case: &CaseReport) {
    let graph = case.baseline_rss_bytes.saturating_add(case.floor_rss_bytes);
    let operating_min = graph.saturating_add(case.processing_rss_min_bytes);
    let operating_max = graph.saturating_add(case.processing_rss_max_bytes);
    println!("{}", case.name);
    line("Operating RSS", &format_range(operating_min, operating_max));
    line("├── Graph Logical", &format_mem(case.graph_logical_bytes));
    line("├── Graph RSS", &format_mem(graph));
    line(
        "└── Processing Memory (RSS)",
        &format_range(case.processing_rss_min_bytes, case.processing_rss_max_bytes),
    );
    line("    ├── Min", &format_mem(case.processing_rss_min_bytes));
    line("    └── Max", &format_mem(case.processing_rss_max_bytes));
    if let (Some(logical_bytes), Some(retained), Some(per)) = (
        case.peak_logical_bytes_held,
        case.retained_rss_bytes,
        case.rss_bytes_per_logical_byte,
    ) {
        line("Peak logical bytes held", &logical_bytes.to_string());
        line("Retained RSS", &format_mem(retained));
        line("RSS bytes per logical byte", &format!("{per} B"));
    }
}

/// Signed MB per extra concurrent query, or `-` when not measured.
fn format_holder(bytes: Option<f64>) -> String {
    match bytes {
        Some(bytes) => format!("{:+.2} MB", bytes / (1024.0 * 1024.0)),
        None => "-".into(),
    }
}

fn line(label: &str, value: &str) {
    println!("{label:<36}{value:>22}");
}

fn format_range(min: u64, max: u64) -> String {
    if min == max {
        format_mem(min)
    } else {
        format!("{} – {}", format_mem(min).trim(), format_mem(max).trim())
    }
}

fn print_workload(workload: &WorkloadReport) {
    print_case(&workload.case);
    println!(
        "reads  succeeded {}  limited {}  failed {}",
        workload.reads_succeeded, workload.reads_limited, workload.reads_failed
    );
    println!(
        "writes  succeeded {}  limited {}  failed {}",
        workload.writes_succeeded, workload.writes_limited, workload.writes_failed
    );
    println!(
        "refusals  connections {}  active-queries {}",
        workload.connections_refused, workload.active_query_refusals,
    );
}

fn emit_case(case: &CaseReport) {
    print_case(case);
    println!();
    let _ = io::stdout().flush();
}

fn emit_workload(workload: &WorkloadReport) {
    print_workload(workload);
    println!();
    let _ = io::stdout().flush();
}

fn controls_from(args: &Args) -> Result<Controls, String> {
    let (read_parts, write_parts) = parse_ratio(&args.read_write_ratio)?;
    let index_mode = parse_index_mode(&args.index_mode)?;
    let read_query = parse_read_query(&args.read_query)?;
    let controls = Controls {
        graph_scale: args.graph_scale,
        workers: args.workers,
        read_parts,
        write_parts,
        max_connections: args.max_connections,
        concurrent_logical_workspace_limit: args.concurrent_logical_workspace_limit,
        ops: args.ops,
        warmup_ops: args.warmup_ops,
        seed: args.workload_seed,
        index_mode,
        read_query,
    };
    controls.validate()?;
    Ok(controls)
}

fn main() {
    let args = Args::parse();
    let controls = match controls_from(&args) {
        Ok(controls) => controls,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let cfg = Config {
        serve_bin: args.serve_bin.clone().unwrap_or_else(default_bolt_bin),
        store: args.uri.clone().unwrap_or_else(default_store),
        out: args.out.clone(),
        controls: controls.clone(),
        skip_probes: args.skip_probes,
        probe: args.probe.clone(),
        calibration_repeats: args.calibration_repeats,
        calibration_hold_ms: args.calibration_hold_ms,
        calibration_family: args.calibration_family.clone(),
    };
    println!(
        "scale {}  workers {}  ratio {}  index {:?}  ops {}  warmup {}",
        controls.graph_scale,
        controls.workers,
        controls.read_write_ratio(),
        match controls.index_mode {
            IndexMode::Indexed => "indexed",
            IndexMode::Scan => "scan",
            IndexMode::Both => "both",
        },
        controls.ops,
        controls.warmup_ops,
    );
    println!(
        "reads  {}",
        match controls.read_query {
            ReadQuery::Mix => "mix",
            ReadQuery::DeepPaths => "deep-paths",
            ReadQuery::SizeSort => "size-sort",
        }
    );
    println!(
        "limits  connections {}  logical bytes {}",
        if controls.max_connections == 0 {
            "unlimited".to_string()
        } else {
            controls.max_connections.to_string()
        },
        if controls.concurrent_logical_workspace_limit == 0 {
            "unlimited".to_string()
        } else {
            controls.concurrent_logical_workspace_limit.to_string()
        },
    );
    println!();
    let _ = io::stdout().flush();
    match run(&cfg, emit_case, emit_workload) {
        Ok(report) => {
            if let Some(calibration) = &report.peak_token_calibration {
                println!("logical-byte calibration");
                println!(
                    "  {:<12} {:>9} {:>6} {:>11} {:>8} {:>9} {:>11} {:>8} {:>9}",
                    "family",
                    "1q slope",
                    "r²",
                    "per holder",
                    "max",
                    "all slope",
                    "cg holder",
                    "cg max",
                    "cg 1q"
                );
                for summary in &calibration.summaries {
                    println!(
                        "  {:<12} {:>9.3} {:>6.3} {:>11} {:>7.3}x {:>9.3} {:>11} {:>8} {:>9}",
                        summary.family,
                        summary.rss_single_holder_slope,
                        summary.rss_single_holder_r_squared,
                        format_holder(summary.rss_per_holder_bytes),
                        summary.max_rss_to_logical_ratio,
                        summary.rss_slope,
                        format_holder(summary.cgroup_per_holder_bytes),
                        summary
                            .max_cgroup_to_logical_ratio
                            .map(|ratio| format!("{ratio:.3}x"))
                            .unwrap_or_else(|| "-".into()),
                        summary
                            .cgroup_single_holder_slope
                            .map(|slope| format!("{slope:.3}"))
                            .unwrap_or_else(|| "-".into()),
                    );
                }
                if let Some(ratio) = calibration.conservative_cgroup_ratio {
                    println!("  conservative observed cgroup ratio {ratio:.3}x");
                }
            }
            if let Some(peak) = report.cgroup_memory_peak_bytes {
                println!("cgroup memory.peak  {}", format_mem(peak));
            }
            println!("wrote {}", args.out.display());
            let _ = io::stdout().flush();
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}
