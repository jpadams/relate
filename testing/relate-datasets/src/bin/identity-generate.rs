use clap::Parser;
use relate_datasets::identity::{generate, IdentityConfig};

#[derive(Parser, Debug)]
#[command(name = "identity-generate", about = "Print planted IAM pattern ids")]
struct Args {
    /// Size vs full generator (`1.0` = 10,000 users)
    #[arg(long, default_value_t = 0.05)]
    scale: f64,
    /// Generator RNG; planted ids are stable for a given seed
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

fn main() {
    let args = Args::parse();
    let g = generate(&IdentityConfig {
        scale: args.scale,
        seed: args.seed,
        ..IdentityConfig::ci()
    });
    println!(
        "users={} groups={} memberships={} nested_admin={} nest={:?} orphan={} sod={} stale={} until={}",
        g.users.len(),
        g.groups.len(),
        g.user_memberships.len() + g.group_memberships.len(),
        g.planted.nested_admin,
        g.planted.nest_groups,
        g.planted.orphan_priv,
        g.planted.sod_user,
        g.planted.stale_grant,
        g.planted.stale_until
    );
}
