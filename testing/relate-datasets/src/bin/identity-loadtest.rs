use std::path::PathBuf;

use clap::Parser;
use relate_datasets::loadtest::{run_identity, CommonArgs, IdentityKnobsArgs};
use relate_datasets::memory::{format_bytes, format_logical_cap};

#[derive(Parser, Debug)]
#[command(
    name = "identity-loadtest",
    about = "Mixed read/write load test on the synthetic IAM identity graph"
)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    #[command(flatten)]
    knobs: IdentityKnobsArgs,
    /// Report JSON path
    #[arg(long, default_value = "target/identity-loadtest.json")]
    out: PathBuf,
}

fn main() {
    let args = Args::parse();
    let cfg = args.common.config(args.knobs.into(), args.out.clone());
    match run_identity(&cfg) {
        Ok(r) => {
            println!(
                "wrote {} ({} measured ops, {} workers, {} usecases)",
                args.out.display(),
                r.config.ops,
                r.config.workers,
                r.usecases.len()
            );
            println!(
                "load {} ms  workers {} ms  total {} ms",
                r.load_duration_ms, r.worker_duration_ms, r.duration_ms
            );
            println!(
                "throughput {:.1} ops/s  writes {:.1}/s ({} ops)  reads {:.1}/s ({} ops)",
                r.throughput.ops_per_sec,
                r.throughput.writes_per_sec,
                r.throughput.write_ops,
                r.throughput.reads_per_sec,
                r.throughput.read_ops
            );
            let start = &r.workload.dataset.start;
            let finish = &r.workload.dataset.finish;
            println!(
                "graph nodes {} → {}, users {} → {}, memberships {} → {}",
                start.nodes,
                finish.nodes,
                start.scalar("users"),
                finish.scalar("users"),
                start.scalar("memberships"),
                finish.scalar("memberships")
            );
            println!(
                "memory RSS {} → {} → {}  graph RSS {} ({:.1} KiB/entity, {:.1} KiB/{})",
                format_bytes(r.memory.after_open.rss_bytes),
                format_bytes(r.memory.start.rss_bytes),
                format_bytes(r.memory.finish.rss_bytes),
                format_bytes(r.memory.graph.rss_bytes),
                r.memory.graph.bytes_per_entity / 1024.0,
                r.memory.graph.bytes_per_unit / 1024.0,
                r.workload.unit
            );
            println!(
                "logical {} (live graph + indexes; RSS includes WAL and allocator)",
                format_logical_cap(r.memory.logical_used_bytes, r.memory.logical_limit_bytes)
            );
            for u in &r.usecases {
                if let Some(l) = &u.latency_ns {
                    println!(
                        "  {:<22} {:>4} ops  p50={:.3}ms p95={:.3}ms errors={}",
                        u.id,
                        u.ops,
                        l.p50 as f64 / 1_000_000.0,
                        l.p95 as f64 / 1_000_000.0,
                        u.errors
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
