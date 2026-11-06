//! The ambient-scrub policy for the e2e spawns: everything a host
//! shell may export that would flip a gated assertion — the
//! agent-harness detection vocabulary (presence vars and substring
//! rules), the row cap, the preview opt-in, the agent-mode override.
//! One cluster, one owner; [`super::cli_command`] and
//! [`super::pty_command`] apply [`scrub_ambient`] to their spawns.

use std::process::Command;

// The one list of agent-harness variables the CLI detects (ADR-0196), mounted
// from the binary's own module so the scrub below can never drift from it.
// The mount reads only the detection vocabulary; the mode resolution and
// the announcement beside it are the binary's — allowed unread here so
// the one-copy mount stays whole.
#[path = "../../src/agent_vars.rs"]
#[allow(dead_code)]
mod agent_vars;

/// Agent-harness environment variables the CLI treats as "driven by an
/// agent". Re-exported by the parent module as the one public path
/// (`support::AGENT_ENV_VARS`).
pub const AGENT_ENV_VARS: &[&str] = agent_vars::PRESENCE_VARS;

/// The non-agent variables no CLI suite may inherit: the row cap and the
/// preview opt-in (ADR-0371; `ontology` gates `db.ontology.*` under
/// ADR-0378).
const AMBIENT_NON_AGENT_VARS: [&str; 3] =
    ["RELATE_MAX_ROWS", "RELATE_PREVIEW", "RELATE_AGENT_MODE"];

/// One neutralization rule over the one ambient vocabulary: a variable
/// is removed outright, or — for the one variable a spawned child may
/// still need — PATH's entries are filtered on the rule's needle. The
/// vocabulary (ADR-0196/ADR-0407; ADR-0426 made the piped default
/// depend on all of it, so a host running its tests from a pi/agent
/// shell — or with a devin/kiro `EDITOR`/`TERM_PROGRAM` — would
/// otherwise flip every piped-default assertion) is enumerated once,
/// here: the presence markers and the `RELATE_*` knobs as
/// removals, then the substring rules' rows — the which-action-per-row
/// fact stated in the partition, not as a magic-string branch inside
/// the scrub loop. The scrub, the guard expectation, and the
/// vocabulary pin below all read this one iterator.
enum AmbientRule {
    Remove(&'static str),
    FilterPath(&'static str),
}

fn ambient_rules() -> impl Iterator<Item = AmbientRule> {
    let removals = AGENT_ENV_VARS
        .iter()
        .copied()
        .chain(AMBIENT_NON_AGENT_VARS)
        .map(AmbientRule::Remove);
    let substring = agent_vars::SUBSTRING_RULES.iter().map(|(name, needle)| {
        if *name == "PATH" {
            AmbientRule::FilterPath(needle)
        } else {
            AmbientRule::Remove(name)
        }
    });
    removals.chain(substring)
}

/// Remove the ambient variables no CLI suite may inherit, and filter
/// PATH's `.pi/agent` entries out (the detector's own
/// [`agent_vars::substring_hit`] match, so a PATH the product would
/// flag is exactly the shape this removes).
/// [`super::cli_command`] applies it to the binary;
/// [`super::pty_command`] applies it to the driver process whose
/// `os.environ` copy becomes the PTY child's environment.
pub(super) fn scrub_ambient(cmd: &mut Command) {
    for name in ambient_scrub_names() {
        cmd.env_remove(name);
    }
    if let Some(path) = fold_path_needles(std::env::var_os("PATH"), &ambient_filter_needles()) {
        cmd.env("PATH", path);
    }
}

/// The shared fold: PATH through every needle — the one conjunction
/// the scrub applies and the wiring guard expects, so the two cannot
/// disagree (a last-write-wins loop would silently diverge from the
/// guarded expectation under a second PATH-based rule).
fn fold_path_needles(
    path: Option<std::ffi::OsString>,
    needles: &[&'static str],
) -> Option<std::ffi::OsString> {
    needles
        .iter()
        .fold(path, |path, needle| path.map(|p| filtered_path(&p, needle)))
}

/// The pure PATH filter: the entries the detector would hit are
/// dropped, the rest re-joined the way the platform spells them — the
/// detector's own case-insensitive [`agent_vars::substring_hit`] match,
/// so a PATH the product would flag is exactly the shape this removes.
fn filtered_path(path: &std::ffi::OsStr, needle: &str) -> std::ffi::OsString {
    std::env::join_paths(
        std::env::split_paths(path)
            .filter(|entry| !agent_vars::substring_hit(&entry.to_string_lossy(), needle)),
    )
    .expect("the scrubbed PATH entries join — none carries a NUL")
}

/// The FilterPath rules' needles, the sibling projection of
/// [`ambient_scrub_names`] over the same one iterator — the wiring
/// guard folds these.
fn ambient_filter_needles() -> Vec<&'static str> {
    ambient_rules()
        .filter_map(|rule| match rule {
            AmbientRule::FilterPath(needle) => Some(needle),
            AmbientRule::Remove(_) => None,
        })
        .collect()
}

/// The names [`scrub_ambient`] removes, from the same iterator it
/// consumes: the rule vocabulary's `Remove` actions (the `FilterPath`
/// rule is a filter, not a removal).
fn ambient_scrub_names() -> Vec<&'static str> {
    ambient_rules()
        .filter_map(|rule| match rule {
            AmbientRule::Remove(name) => Some(name),
            AmbientRule::FilterPath(_) => None,
        })
        .collect()
}

/// The names `cmd` records as removed (an explicit set would show as
/// `Some`).
fn removed_envs(cmd: &Command) -> Vec<&std::ffi::OsStr> {
    cmd.get_envs()
        .filter_map(|(name, value)| value.is_none().then_some(name))
        .collect()
}

fn assert_removed(cmd: &Command, names: &[&str], builder: &str) {
    let removed = removed_envs(cmd);
    for name in names {
        assert!(
            removed
                .iter()
                .any(|removed| removed.to_str() == Some(*name)),
            "{name} must be removed by {builder}; removed: {removed:?}"
        );
    }
}

/// [`super::cli_command`] owns the ambient scrub for the binary (see
/// its doc): drop a name and a developer shell's export reaches every
/// `cli_command`-built spawn, flipping a gate or truncating rows an
/// assertion counts.
#[test]
fn cli_command_removes_the_ambient_scrub() {
    assert_removed(&super::cli_command(), &ambient_scrub_names(), "cli_command");
}

/// [`super::pty_command`] owns the same scrub for the PTY driver
/// process, whose environment the PTY child inherits, plus the
/// telemetry axes: drop either and a spawn that never builds a
/// `cli_command` inherits the host's value.
#[test]
fn pty_command_removes_the_ambient_scrub_and_telemetry_axes() {
    let cmd = super::pty_command("pty_env_probe.py");
    let mut names = ambient_scrub_names();
    names.extend(super::TELEMETRY_AXES);
    assert_removed(&cmd, &names, "pty_command");
}

/// The FilterPath wiring has a guard too, not just the pure filter:
/// the built command's PATH is the ambient PATH with every FilterPath
/// rule's needle entries dropped — the expectation folds all the
/// needles, the same conjunction the scrub itself applies, so it holds
/// for any number of such rules and a wiring regression (wrong needle,
/// or the filtered PATH never applied) leaves this red while the
/// Remove guards stay green.
#[test]
fn cli_command_filters_the_ambient_path() {
    let needles = ambient_filter_needles();
    assert!(!needles.is_empty(), "the vocabulary has a FilterPath rule");
    let cmd = super::cli_command();
    let recorded = cmd
        .get_envs()
        .find(|(name, _)| *name == "PATH")
        .and_then(|(_, value)| value)
        .expect("PATH is set on the built command");
    let expected = fold_path_needles(std::env::var_os("PATH"), &needles);
    assert_eq!(Some(recorded), expected.as_deref());
}

/// The pure filter: a `.pi/agent` entry is dropped and a clean entry
/// kept, case-insensitively — the detector's own match.
#[test]
fn filtered_path_drops_the_needle_entries() {
    let path = std::env::join_paths(["/usr/bin", "/home/u/.pi/agent/bin"])
        .expect("the fixture PATH joins");
    let filtered = filtered_path(&path, ".pi/agent");
    let entries: Vec<std::path::PathBuf> = std::env::split_paths(&filtered).collect();
    assert_eq!(entries, vec![std::path::PathBuf::from("/usr/bin")]);
}

/// The vocabulary's non-agent names and the substring rules' removable
/// names are a product contract, not an implementation detail: the
/// scrub must remove exactly the row cap, the preview opt-in, the
/// agent-mode override, `EDITOR` and `TERM_PROGRAM`. Spelled literally
/// here, independently of the removal lists, so deleting a name from
/// the vocabulary cannot silently shrink both the removal and the
/// derived expectation the guards above check it against.
#[test]
fn ambient_scrub_vocabulary_pins_the_non_agent_names() {
    let mut expected = AGENT_ENV_VARS.to_vec();
    expected.extend([
        "RELATE_MAX_ROWS",
        "RELATE_PREVIEW",
        "RELATE_AGENT_MODE",
        "EDITOR",
        "TERM_PROGRAM",
    ]);
    assert_eq!(
        ambient_scrub_names(),
        expected,
        "the ambient scrub vocabulary must keep the row cap, the preview opt-in, \
         the agent-mode override, and the substring rules' removable names"
    );
}
