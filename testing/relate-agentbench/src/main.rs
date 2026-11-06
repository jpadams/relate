//! The headless agent A/B harness (ADR-0404 principle 5, LABS-481 G-3):
//! runs a fixed question set through headless Claude Code sessions
//! (`claude -p`), one arm per Relate output mode, and measures what
//! DuckDB's Agent Mode experiment measured — the tokens the agent reads
//! from tool output — plus turns, wall time, and answer correctness.
//!
//! The arms differ only in the PATH wrapper the session's `relate`
//! resolves to: every wrapper injects one `--mode` flag — the baseline
//! pins the pre-ADR-0426 machine default (`jsonl`), each comparison
//! arm its mode. The agent never knows; the bytes it reads are the
//! bytes that mode produces.
//!
//! The pieces: [`questions`] (the question set and the correctness
//! gate), [`transcript`] (the session stream's typed contract),
//! [`arms`] (the arm vocabulary and the PATH wrappers), [`harness`]
//! (seed, session, measure), [`report`] (the p25/median/p75
//! distributions table).

mod arms;
mod harness;
mod questions;
mod report;
mod transcript;

use std::path::PathBuf;

use clap::Parser;

use crate::arms::Arm;

#[derive(Parser)]
#[command(
    name = "agentbench",
    about = "Headless agent A/B: tool-output tokens per Relate output mode",
    version
)]
struct Cli {
    #[arg(long)]
    /// The questions file (JSON array; see docs/testing/agentbench.md).
    questions: PathBuf,
    #[arg(long)]
    /// The database directory to (re)create before every measured run.
    database: PathBuf,
    /// The arms to run: `baseline` (the pre-ADR-0426 machine default,
    /// pinned `--mode jsonl` because a pass-through would resolve to
    /// the harness sessions' own detected default) plus any comparison
    /// mode, e.g. `--arms baseline,toon` — each entry is `baseline` or
    /// a `--mode` name or alias (resolved through the CLI's own mode
    /// vocabulary; anything else is a usage error).
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "baseline,toon",
        value_parser = Arm::parse
    )]
    arms: Vec<Arm>,
    #[arg(long, default_value = "1")]
    /// Repetitions per question × arm (the report shows their
    /// p25/median/p75 distributions); at least 1 — zero runs are not a
    /// measurement.
    reps: usize,
    #[arg(long, default_value = "30")]
    /// The headless session's per-run turn cap.
    max_turns: u64,
    /// The headless `claude` binary (default: `claude` on PATH).
    #[arg(long, default_value = "claude")]
    claude: PathBuf,
    /// The `relate` binary the wrappers exec (default:
    /// `RELATE_BIN`, the workspace's testing-harness knob —
    /// workspace-relative or absolute).
    #[arg(long)]
    relate: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();
    let questions = questions::load(&cli.questions);
    let arms = cli.arms;
    assert!(!arms.is_empty(), "--arms needs at least one arm");
    assert!(
        cli.reps >= 1,
        "--reps must be at least 1 — zero runs are not a measurement"
    );
    // Duplicate arms would pool two runs' rows into one report row.
    // The guard compares resolved modes (`Arm::mode`), so two labels of
    // one mode — `baseline`+`jsonl` as well as `md`+`markdown` — are
    // caught as the duplicate they are.
    for (index, arm) in arms.iter().enumerate() {
        assert!(
            !arms[..index]
                .iter()
                .any(|earlier| earlier.mode() == arm.mode()),
            "duplicate arm {} — two entries resolve to the same mode ({}), and \
             their runs would pool into one report row",
            arm.label(),
            arm.mode().name()
        );
    }
    let claude = cli.claude;
    let relate = cli.relate.unwrap_or_else(default_relate);
    // The wrapper bakes the binary's path into a generated script, and
    // the agent's cwd changes mid-session — an absolute, normalized path
    // is the only one that keeps resolving, for a relative or absolute
    // input alike (a bad value fails loudly here, not mid-session).
    let relate = std::fs::canonicalize(&relate).unwrap_or_else(|e| {
        panic!(
            "the relate binary at {} does not resolve: {e}",
            relate.display()
        )
    });

    // The wrapper root: an unpredictable temp dir (the session resolves
    // `relate` from a PATH that puts it first), one arm subdir each,
    // removed when it drops at the end of main. The bench carries its
    // path (the TempDir itself stays alive here for the whole run).
    let wrapper_root = tempfile::tempdir().expect("the wrapper root is a creatable temp dir");
    let bench = harness::Bench::new(
        claude,
        relate,
        cli.database,
        cli.max_turns,
        wrapper_root.path(),
    );
    let mut stats = Vec::new();
    for arm in &arms {
        arms::write_wrapper(&bench.arm_dir(arm), arm, &bench.relate);
        for question in &questions {
            for rep in 1..=cli.reps {
                let run = harness::run_once(&bench, arm, question, rep);
                eprintln!(
                    "{} {} rep {}: {} tool-output bytes, {} input tokens, correct={}",
                    arm.label(),
                    question.id,
                    rep,
                    run.tool_output_bytes,
                    run.input_tokens,
                    run.correct
                );
                stats.push(run);
            }
        }
    }
    report::report(&arms, &stats);
}

/// The default `relate` binary: the workspace's one testing-harness
/// knob, `RELATE_BIN`, resolved by the shared helper
/// (`relate-test-support`'s `relate_bin`, the same resolution
/// the graph-examples pulls use) — a loud failure with the build
/// instructions, never a sniff of `target/` dirs.
fn default_relate() -> PathBuf {
    relate_test_support::relate_bin().unwrap_or_else(|| {
        panic!(
            "{} (or pass --relate)",
            relate_test_support::relate_bin_missing_message()
        )
    })
}
