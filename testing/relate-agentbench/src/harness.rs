//! The measurement itself: one run — seed, session, measure — and the
//! run's numbers.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::arms::Arm;
use crate::questions::Question;
use crate::transcript::{last_text_block, parse_line, Block, Event, Part, ToolContent, Usage};

/// One measured run: the numbers DuckDB's experiment reported, per run.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct RunStats {
    pub(crate) question: String,
    pub(crate) arm: Arm,
    pub(crate) rep: usize,
    /// The metric: bytes of tool output the agent read (the sum of every
    /// Bash tool_result's content in the session's event stream).
    pub(crate) tool_output_bytes: usize,
    /// The session's total input tokens (what the model actually paid to
    /// re-read context, tool output included, each turn).
    pub(crate) input_tokens: u64,
    /// The session's total output tokens.
    pub(crate) output_tokens: u64,
    pub(crate) turns: u64,
    pub(crate) seconds: f64,
    pub(crate) correct: bool,
}

/// The bench's fixed configuration, built once in main: the binaries,
/// the database, the turn cap, and the wrapper root the arms'
/// wrappers live under.
pub(crate) struct Bench {
    pub(crate) claude: PathBuf,
    pub(crate) relate: PathBuf,
    pub(crate) database: PathBuf,
    pub(crate) max_turns: u64,
    wrapper_root: PathBuf,
}

impl Bench {
    /// The bench's fixed configuration, from the parsed CLI plus the
    /// live wrapper root.
    pub(crate) fn new(
        claude: PathBuf,
        relate: PathBuf,
        database: PathBuf,
        max_turns: u64,
        wrapper_root: &Path,
    ) -> Bench {
        Bench {
            claude,
            relate,
            database,
            max_turns,
            wrapper_root: wrapper_root.to_path_buf(),
        }
    }

    /// The arm's wrapper dir, `<wrapper root>/<arm label>` — the one
    /// derivation of the layout, for both the wrapper writer and the
    /// session's PATH.
    pub(crate) fn arm_dir(&self, arm: &Arm) -> PathBuf {
        self.wrapper_root.join(arm.label())
    }
}

/// One measured run, composed: seed the database to identical state,
/// run the headless session against the arm's wrapper dir, timed, then
/// measured — the run's unit lives here, not in the caller's loop.
/// Seeding sits inside the run on purpose: the agent's Bash tool is
/// unrestricted, so a run that writes to the store must not contaminate
/// the next rep's measurement — every measured run starts from
/// identical state.
pub(crate) fn run_once(bench: &Bench, arm: &Arm, question: &Question, rep: usize) -> RunStats {
    let arm_dir = bench.arm_dir(arm);
    seed_database(&bench.relate, &bench.database, &question.setup_cypher);
    let started = Instant::now();
    let events = run_headless(
        &bench.claude,
        &arm_dir,
        question,
        bench.max_turns,
        &bench.database,
    );
    let seconds = started.elapsed().as_secs_f64();
    measure(&events, question, arm, rep, seconds)
}

/// Recreate the database and run the question's setup Cypher through the
/// real binary (never a wrapper — seeding is the harness's own work,
/// not the agent's).
pub(crate) fn seed_database(relate: &Path, database: &Path, setup: &str) {
    // Identical start state is the invariant the measurement rests on, so
    // only "did not exist" is ignorable — any other removal failure is
    // loud, never seeded-over stale state.
    match std::fs::remove_dir_all(database) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!(
            "the previous database at {} is not removable: {e}",
            database.display()
        ),
    }
    let mut command = Command::new(relate);
    // The canonical harness child-env policy (ADR-0079 §6): the seeding
    // run is configured only by its argv — an ambient RELATE_* knob
    // from the operator's shell must not leak into the state every
    // measured run starts from.
    relate_test_support::harness_child_env(&mut command);
    // Stderr is piped, not nulled: a failing setup Cypher names its error
    // in the assert below instead of a bare exit status.
    let output = command
        .arg("--database")
        .arg(database)
        .arg(setup)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("the real binary seeds the database");
    assert!(
        output.status.success(),
        "the setup Cypher failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// One headless session: `claude -p` with the Bash tool only, the arm's
/// wrapper dir first on PATH, and the session transcript as typed
/// events. The prompt steers the agent to the CLI without naming the
/// mode — the arms must differ only in the bytes the CLI emits.
pub(crate) fn run_headless(
    claude: &Path,
    wrapper_dir: &Path,
    question: &Question,
    max_turns: u64,
    database: &Path,
) -> Vec<Event> {
    let prompt = format!(
        "Use the `relate` CLI (already on PATH) with the database \
         at {} to answer this question. Read the CLI's own output for the \
         answer; do not guess. Question: {}",
        database.display(),
        question.prompt
    );
    let path = std::env::var_os("PATH")
        .expect("PATH is set — the session resolves the arm's wrapper through it");
    let mut command = Command::new(claude);
    // The measured environment is defined: the same harness child-env
    // policy scrubs ambient RELATE_* knobs from the session, and its
    // Bash children (the wrappers) inherit the scrub — so the arms differ
    // only in the wrapper-injected `--mode`, never in a leaked ambient
    // knob that would confound the A/B.
    relate_test_support::harness_child_env(&mut command);
    let output = command
        .arg("-p")
        .arg(&prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--max-turns")
        .arg(max_turns.to_string())
        .arg("--allowedTools")
        .arg("Bash")
        .env("PATH", session_path(wrapper_dir, &path))
        .output()
        .expect("the headless session runs");
    assert!(
        output.status.success(),
        "the session failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("the session transcript is UTF-8")
        .lines()
        .map(parse_line)
        .collect()
}

/// The session's PATH: the arm's wrapper dir prepended to the ambient
/// PATH, joined the way the platform spells PATH entries.
fn session_path(wrapper_dir: &Path, path: &std::ffi::OsStr) -> std::ffi::OsString {
    std::env::join_paths(
        std::iter::once(wrapper_dir.to_path_buf()).chain(std::env::split_paths(path)),
    )
    .expect("the PATH entries join — none carries the path-list separator")
}

/// Measure one run from its event stream: the DuckDB experiment's
/// numbers — tool-output bytes (the metric), the session's canonical
/// token totals (the result event's), the turns — plus the correctness
/// gate.
pub(crate) fn measure(
    events: &[Event],
    question: &Question,
    arm: &Arm,
    rep: usize,
    seconds: f64,
) -> RunStats {
    let mut tool_output_bytes = 0;
    let mut turns = 0;
    // The canonical totals are the result event's — nothing is summed,
    // so a stream with no result event fails loudly at the extraction
    // below instead of reporting a fabricated zero.
    let mut totals: Option<&Usage> = None;
    let mut last_text: &str = "";
    for event in events {
        match event {
            Event::User { message } => {
                tool_output_bytes += tool_result_bytes(message.content.as_deref());
            }
            Event::Assistant { message } => {
                turns += 1;
                if let Some(text) = last_text_block(message.content.as_deref()) {
                    last_text = text;
                }
            }
            // The result event is the run's summary: its text is the
            // answer the gate reads, its `usage` the canonical totals.
            Event::Result { result, usage } => {
                totals = Some(usage);
                if let Some(text) = result {
                    last_text = text;
                }
            }
            Event::Unknown => {}
        }
    }
    let totals = totals.expect(
        "the session's result event — the canonical token totals — is missing: \
         not a clean measurement",
    );
    RunStats {
        question: question.id.clone(),
        arm: arm.clone(),
        rep,
        tool_output_bytes,
        input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        turns,
        seconds,
        correct: crate::questions::answer_is(last_text, &question.expected_answer),
    }
}

/// The bytes of tool output in one message's content: a str content is
/// its length, a parts content the sum of its text parts — and any
/// other part shape fails loudly, never a silently-zeroed count.
fn tool_result_bytes(content: Option<&[Block]>) -> usize {
    content
        .into_iter()
        .flatten()
        .filter_map(|block| match block {
            Block::ToolResult { content } => Some(match content {
                ToolContent::Str(text) => text.len(),
                ToolContent::Parts(parts) => parts
                    .iter()
                    .map(|part| match part {
                        Part::Text { text } => text.len(),
                        Part::Unknown => panic!(
                            "a non-text part in a tool result — the byte metric does not know how to read it"
                        ),
                    })
                    .sum(),
            }),
            _ => None,
        })
        .sum()
}
