use clap::Parser;
use relate_datasets::fraud::{generate, FraudConfig};

#[derive(Parser, Debug)]
#[command(name = "fraud-generate", about = "Print planted fraud pattern ids")]
struct Args {
    /// Size vs full generator (`1.0` = 10,000 accounts)
    #[arg(long, default_value_t = 0.05)]
    scale: f64,
    /// Generator RNG; planted ids are stable for a given seed
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

fn main() {
    let args = Args::parse();
    let g = generate(&FraudConfig {
        scale: args.scale,
        seed: args.seed,
        ..FraudConfig::ci()
    });
    println!(
        "accounts={} transfers={} fanout={} k={} cycle={:?}",
        g.accounts.len(),
        g.transfers.len(),
        g.planted.fanout_source,
        g.planted.fanout_k,
        g.planted.cycle_accounts
    );
}
