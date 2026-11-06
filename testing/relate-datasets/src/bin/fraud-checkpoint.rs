use std::path::PathBuf;

use clap::Parser;
use relate_datasets::checkpoint::{run, CheckpointConfig};
use relate_datasets::memory::format_bytes;

#[derive(Parser, Debug)]
#[command(
    name = "fraud-checkpoint",
    about = "Load the fraud graph, checkpoint, and report logical / RSS / snapshot file sizes"
)]
struct Args {
    /// Database URI (`:memory:`, path, or `file://`). `:memory:` still reports in-process snapshot size
    #[arg(long, default_value = "./target/fraud-checkpoint")]
    uri: String,
    /// Graph size vs full generator (`1.0` = 10,000 accounts)
    #[arg(long, default_value_t = 0.05)]
    scale: f64,
    /// RNG for the synthetic fraud graph
    #[arg(long, default_value_t = 42)]
    graph_seed: u64,
    /// Planted mule out-degree (`fanout-src` destinations)
    #[arg(long, default_value_t = 12)]
    fanout_k: usize,
    /// Planted directed cycle length (`cycle-0` …)
    #[arg(long, default_value_t = 4)]
    cycle_len: usize,
    /// Local WAL directory when `--uri` is persistent
    #[arg(long)]
    wal_dir: Option<PathBuf>,
    /// Delete the existing database at `--uri` (and `--wal-dir` if set) before load. No-op for `:memory:`
    #[arg(long)]
    reset: bool,
    /// Live graph + index cap: integer bytes, or a size with mb/mib/gb/gib (RELATE_LOGICAL_GRAPH_SIZE_LIMIT; 0 or omit is unlimited)
    #[arg(long, value_name = "SIZE", value_parser = parse_cli_size)]
    logical_graph_size_limit: Option<u64>,
    /// Optional JSON report path
    #[arg(long)]
    out: Option<PathBuf>,
}

fn parse_cli_size(s: &str) -> std::result::Result<u64, String> {
    relate::parse_memory_limit(s).map_err(|e| e.to_string())
}

fn main() {
    let args = Args::parse();
    let cfg = CheckpointConfig {
        uri: args.uri,
        scale: args.scale,
        graph_seed: args.graph_seed,
        fanout_k: args.fanout_k,
        cycle_len: args.cycle_len,
        wal_dir: args.wal_dir,
        reset: args.reset,
        logical_graph_size_limit: args.logical_graph_size_limit,
        out: args.out.clone(),
    };
    match run(&cfg) {
        Ok(r) => {
            println!(
                "graph nodes {}  accounts {}  transfers {}",
                r.nodes, r.accounts, r.transfers
            );
            println!(
                "logical {}  rss {}  files {}",
                format_bytes(r.logical_used_bytes),
                format_bytes(r.rss_bytes),
                format_bytes(r.data_file_bytes)
            );
            if let Some(out) = args.out {
                println!("wrote {}", out.display());
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
