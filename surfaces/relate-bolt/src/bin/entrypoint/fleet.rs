//! `INSTANCES=N` fleet supervision: N `relate serve` processes in one
//! container, ports stepping from the per-surface bases, each seeded into
//! its own suffixed URI.
//!
//! ADR-0233: supervision is sync — `std::process::Command` children reaped
//! by `try_wait` polling, the shutdown signal arriving over an mpsc from
//! signal-hook's blocking iterator thread. The `tokio::select!` becomes a
//! `recv_timeout`.

use std::sync::mpsc;
use std::time::Duration;

use crate::args::{
    note_dropped_legacy_args, resolve_plan, serve_argv, warn_if_loopback, ServePlan,
};
use crate::seed::{seed_policy, seed_unless_exists};
use crate::{prepare_instance, BIN_DIR};

/// How often the supervisor re-polls the children for exits while waiting
/// for a shutdown signal. Detection latency only; no work happens per tick.
const REAP_POLL: Duration = Duration::from_millis(50);

/// `INSTANCES=N`: spawn N `relate serve` processes; every surface's
/// port steps from its base, instance i getting base+i−1 (ADR-0249). The
/// bases: `--bolt`/`--http`/`--mcp` when given; an unset `--http`/`--mcp`
/// base derives from the bolt base so the three per-surface ranges can
/// never overlap — bolt B..B+N−1, http B+N..B+2N−1, mcp B+2N..B+3N−1
/// (the adjacent single-run defaults 7474/7475 would collide under
/// stepping). Explicit bases must be pairwise N apart, a loud error
/// otherwise. Each persistent URI is suffixed per instance (`graph` →
/// `graph-1`, `graph.rl` → `graph-1.rl`); `:memory:` stays per-process.
/// Each instance seeds its own URI. A test fleet: `docker run
/// -p 7687-7692:7687-7692 -e INSTANCES=2 …` (bolt 7687-7688, http
/// 7689-7690, mcp 7691-7692).
/// Teardown: PID 1 installs SIGINT/SIGTERM handlers (below), kills every
/// child, and exits 130/143 — `docker rm -f`'s SIGKILL is the last resort,
/// and the fsync'd WAL makes even that data-safe.
/// The fleet's per-surface port bases (bolt, http, mcp): explicit flags
/// win; an unset `--http`/`--mcp` base derives from the bolt base so the
/// three stepped ranges can never overlap (see [`serve_fleet`]). Overflow
/// and overlapping explicit bases are loud errors.
fn fleet_bases(
    args: &[String],
    instances: usize,
    bolt_base: u16,
) -> Result<(u16, u16, u16), String> {
    let n = instances as u32;
    let bolt_base = u32::from(bolt_base);
    let explicit = |flag: &str| crate::args::explicit_port(args, flag).map(|p| p.map(u32::from));
    let http_base = match explicit("--http")? {
        Some(p) => p,
        None => bolt_base + n,
    };
    let mcp_base = match explicit("--mcp")? {
        Some(p) => p,
        None => bolt_base + 2 * n,
    };
    let ranges = [
        ("--bolt", bolt_base, bolt_base + n - 1),
        ("--http", http_base, http_base + n - 1),
        ("--mcp", mcp_base, mcp_base + n - 1),
    ];
    for (flag, _, last) in ranges {
        if last > u32::from(u16::MAX) {
            return Err(format!(
                "INSTANCES={instances} overflows the {flag} port range"
            ));
        }
    }
    for (i, (flag_a, lo_a, hi_a)) in ranges.iter().enumerate() {
        for (flag_b, lo_b, hi_b) in &ranges[i + 1..] {
            if lo_a <= hi_b && lo_b <= hi_a {
                return Err(format!(
                    "INSTANCES={instances} makes the {flag_a} ({lo_a}..={hi_a}) and {flag_b} ({lo_b}..={hi_b}) port ranges overlap; set them at least {n} apart"
                ));
            }
        }
    }
    Ok((
        u16::try_from(bolt_base).expect("a u16 base"),
        u16::try_from(http_base).expect("range checked"),
        u16::try_from(mcp_base).expect("range checked"),
    ))
}

pub(crate) fn serve_fleet(args: &[String], instances: usize) {
    note_dropped_legacy_args(args);
    let plan = match resolve_plan(args) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("entrypoint: {e}");
            std::process::exit(1);
        }
    };
    // Same guard as the single-serve path: a loopback fleet is silently
    // unreachable from the host — warn before spawning anything.
    warn_if_loopback(&plan.host);
    let bases = match fleet_bases(args, instances, plan.bolt_port) {
        Ok(bases) => bases,
        Err(e) => {
            eprintln!("entrypoint: {e}");
            std::process::exit(1);
        }
    };

    let (config, force_seed) = seed_policy();

    // Each instance seeds its own suffixed URI before its own server
    // spawns; the per-instance URIs are disjoint, so a live sibling never
    // touches the store being seeded.
    let mut children = Vec::with_capacity(instances);
    for i in 1..=instances {
        let offset = u16::try_from(i - 1).expect("instance fits u16");
        let instance_plan = ServePlan {
            host: plan.host.clone(),
            // base + N - 1 is in range per fleet_bases, so base + offset is.
            bolt_port: bases.0 + offset,
            http_port: bases.1 + offset,
            mcp_port: bases.2 + offset,
        };
        let instance_uri = prepare_instance(args, Some(i));
        seed_unless_exists(
            &instance_uri,
            &format!("instance {i} ({instance_uri})"),
            &config,
            force_seed,
        );
        let argv = serve_argv(args, &instance_plan, &instance_uri);
        let child = std::process::Command::new(format!("{BIN_DIR}/relate"))
            .args(&argv)
            .spawn()
            .unwrap_or_else(|e| {
                eprintln!(
                    "entrypoint: instance {i} failed to start (bolt {}): {e}",
                    instance_plan.bolt_port
                );
                std::process::exit(1);
            });
        let label = format!(
            "{} (bolt {}, http {}, mcp {})",
            plan.host, instance_plan.bolt_port, instance_plan.http_port, instance_plan.mcp_port
        );
        eprintln!("entrypoint: instance {i} listening on {label} (uri={instance_uri})");
        children.push((i, label, child));
    }

    // The entrypoint is PID 1 on the fleet path, and the kernel ignores
    // unhandled signals for PID 1, so Ctrl-C and `docker stop` need these
    // explicit handlers. The signal thread blocks in signal-hook and reports
    // over the channel; the supervisor's `recv_timeout` is the old select!.
    let (sig_tx, sig_rx) = mpsc::channel::<i32>();
    std::thread::spawn(move || {
        let sig = relate_serve::signal::wait_or_exit("entrypoint");
        let _ = sig_tx.send(sig);
    });

    let mut exit_code = 0;
    let mut by_signal = None;
    loop {
        match sig_rx.recv_timeout(REAP_POLL) {
            Ok(sig) => {
                by_signal = Some(relate_bolt::signal::signal_exit_code(sig));
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
        let mut i = 0;
        while i < children.len() {
            match children[i].2.try_wait() {
                Ok(Some(status)) => {
                    let (instance, bind_i, _) = children.swap_remove(i);
                    let code = status.code().unwrap_or(1);
                    if code != 0 && exit_code == 0 {
                        exit_code = code;
                    }
                    eprintln!("entrypoint: instance {instance} on {bind_i} exited: {status}");
                }
                Ok(None) => i += 1,
                Err(e) => {
                    let (instance, bind_i, _) = children.swap_remove(i);
                    if exit_code == 0 {
                        exit_code = 1;
                    }
                    eprintln!("entrypoint: instance {instance} on {bind_i}: wait error: {e}");
                }
            }
        }
        if children.is_empty() {
            eprintln!("entrypoint: all instances exited");
            break;
        }
    }
    if let Some(code) = by_signal {
        eprintln!("entrypoint: signal received, stopping the fleet");
        // Kill every child, then reap: the fsync'd WAL makes even a SIGKILL
        // data-safe.
        for (_, _, child) in &mut children {
            let _ = child.kill();
        }
        for (_, _, child) in &mut children {
            let _ = child.wait();
        }
        std::process::exit(code);
    }
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::fleet_bases;

    fn sv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unset_http_mcp_derive_non_overlapping_bases_from_the_bolt_base() {
        // The adjacent single-run defaults 7474/7475 would collide under
        // stepping; derivation guarantees disjoint ranges.
        assert_eq!(fleet_bases(&sv(&[]), 2, 7687).unwrap(), (7687, 7689, 7691));
        assert_eq!(fleet_bases(&sv(&[]), 3, 7687).unwrap(), (7687, 7690, 7693));
        assert_eq!(fleet_bases(&sv(&[]), 1, 7687).unwrap(), (7687, 7688, 7689));
    }

    #[test]
    fn explicit_bases_win_and_overlap_is_a_loud_error() {
        let args = sv(&["--http", "17474", "--mcp", "27474"]);
        assert_eq!(fleet_bases(&args, 2, 7687).unwrap(), (7687, 17474, 27474));
        // Explicit bases closer than N overlap once stepped.
        let args = sv(&["--http", "7688"]);
        let err = fleet_bases(&args, 2, 7687).unwrap_err();
        assert!(err.contains("overlap"), "{err}");
        // A bad port value names its flag.
        let args = sv(&["--mcp", "abc"]);
        assert!(fleet_bases(&args, 2, 7687).unwrap_err().contains("--mcp"));
    }

    #[test]
    fn range_overflow_is_a_loud_error() {
        let err = fleet_bases(&sv(&[]), 2, 65535).unwrap_err();
        assert!(err.contains("overflows"), "{err}");
    }
}
