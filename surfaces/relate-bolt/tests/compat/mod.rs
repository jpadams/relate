//! The Bolt compatibility script harness (ADR-0304): boltkit's `.bolt`
//! stub-server scripts turned around. The harness is the client: it sends
//! a script's `C:` lines to a real `relate-bolt` process and checks
//! the server's replies against the `S:` lines. `README.md` beside this
//! file is the script reference.
//!
//! `parser` lowers a script to wire steps, `runner` replays them, `value`
//! holds the JSON ↔ PackStream mapping and the matching rules, `messages`
//! the message-name vocabulary, and `hex` the byte-string notation.

mod hex;
mod messages;
mod parser;
mod runner;
mod value;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Scripts replayed at once, each against its own server process.
const WORKERS: usize = 4;

/// A script that failed to read, parse or replay, located at
/// `path:line` when a line is to blame.
pub(crate) struct ScriptError {
    path: PathBuf,
    line: Option<usize>,
    message: String,
}

impl ScriptError {
    fn new(path: &Path, line: Option<usize>, message: String) -> Self {
        Self {
            path: path.to_path_buf(),
            line,
            message,
        }
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.path.display(), self.message),
            None => write!(f, "{}: {}", self.path.display(), self.message),
        }
    }
}

/// Every `*.bolt` file under `dir`, recursively, in the sorted entry order
/// of the shared tree walker; at least one must exist.
pub(crate) fn discover(dir: &Path) -> Vec<PathBuf> {
    let found: Vec<PathBuf> = relate_test_support::walk_tree(dir, |path, _| path.to_path_buf())
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "bolt"))
        .collect();
    // walk_tree yields nothing for a missing root: name the directory.
    assert!(
        !found.is_empty(),
        "no .bolt scripts under {}",
        dir.display()
    );
    found
}

/// Replay every script, each against its own server; return every failure
/// (`path:line`, the script line, expected and actual), sorted by location.
/// The caller reports them at its `#[test]` boundary.
pub(crate) fn run_scripts(paths: &[PathBuf]) -> Vec<ScriptError> {
    assert!(!paths.is_empty(), "no .bolt scripts to run");
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(paths.len()) {
            scope.spawn(|| {
                while let Some(path) = paths.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Err(e) = run_file(path) {
                        failures
                            .lock()
                            .expect("no worker panics holding the lock")
                            .push(e);
                    }
                }
            });
        }
    });
    let mut failures = failures
        .into_inner()
        .expect("no worker panics holding the lock");
    failures.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    failures
}

fn run_file(path: &Path) -> Result<(), ScriptError> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| ScriptError::new(path, None, format!("cannot read script: {e}")))?;
    runner::run(&parser::parse(path, &source)?)
}
