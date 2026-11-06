//! The A/B arms and the PATH wrappers that inject each arm's output
//! mode onto the session's `relate` — the one mechanism that
//! differs between arms.

use std::path::Path;

use relate_formatter::Mode;

/// One A/B arm: the report label and the resolved `--mode` its wrapper
/// injects — one pair of facts carried as data from the parse (clap's
/// `value_parser`, resolved through the formatter's one mode
/// vocabulary, so a typo'd arm is a usage error here — not ten measured
/// sessions of `invalid mode` tool output — and an alias reports its
/// canonical spelling). `baseline` is the pre-ADR-0426 machine default
/// (`jsonl`) under its historical label — pinned explicitly, because
/// this harness's own sessions run detected and a pass-through would
/// resolve to the detected default (`toon`), making both arms measure
/// the same bytes; `baseline` and `jsonl` are one arm under two labels,
/// exactly as `md` and `markdown` are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Arm {
    label: &'static str,
    mode: Mode,
}

impl Arm {
    /// One `--arms` entry: `baseline`, or the name (or alias) of a
    /// `--mode` value (e.g. `toon`, `md`).
    pub(crate) fn parse(entry: &str) -> Result<Arm, String> {
        // `baseline` folds by its own comparison and `Mode::parse` folds
        // its input, so one case policy — the vocabulary's — covers the
        // whole entry without a pre-fold here (the error still names the
        // spelling the operator typed).
        if entry.eq_ignore_ascii_case("baseline") {
            return Ok(Arm {
                label: "baseline",
                mode: Mode::Jsonl,
            });
        }
        let mode = Mode::parse(entry).ok_or_else(|| {
            format!(
                "arm '{entry}' is neither 'baseline' nor a --mode value (one of: {})",
                Mode::names()
            )
        })?;
        Ok(Arm {
            label: mode.name(),
            mode,
        })
    }

    /// The arm's name in the report and the per-run rows.
    pub(crate) fn label(&self) -> &'static str {
        self.label
    }

    /// The arm's mode — the one fact the wrapper's inject and the
    /// duplicate guard both read.
    pub(crate) fn mode(&self) -> Mode {
        self.mode.clone()
    }
}

/// The run rows keep the label spelling they always had.
impl serde::Serialize for Arm {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

/// Quote one word for POSIX sh: single quotes with the `'\''` escape.
/// The wrapper script interpolates the binary path, and Rust's `{:?}`
/// is not a shell-quoting rule — `$`, backticks, and spaces must not
/// survive into the generated script.
fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The arm's `relate` wrapper: one `--mode` value — the arm's
/// [`Arm::mode`] — injected **before** the caller's arguments. Before,
/// not after (pinned empirically against the real CLI): after the
/// caller's args the injected flag lands past the `--` separator or in
/// a lifecycle verb's own parser — `relate info` rejects it — and
/// either breaks an unrestricted agent session in every arm. The
/// trade it accepts: the CLI's mode slot is last-wins, so an agent that
/// passes its own `--mode` overrides the arm — accepted because
/// nothing in the session's prompt suggests a mode flag, and a session
/// that did would surface as a byte-count anomaly in the report. The
/// mode name comes from the formatter's fixed alphanumeric vocabulary,
/// so it needs no quoting; the binary path is
/// single-quoted ([`shell_quote`]).
pub(crate) fn write_wrapper(arm_dir: &Path, arm: &Arm, relate: &Path) {
    std::fs::create_dir_all(arm_dir).expect("the arm wrapper dir");
    let script = arm_dir.join("relate");
    let inject = format!(" --mode {}", arm.mode().name());
    let bin = shell_quote(relate.to_str().expect("the relate binary path is UTF-8"));
    std::fs::write(&script, format!("#!/bin/sh\nexec {bin}{inject} \"$@\"\n"))
        .expect("the wrapper script");
    make_executable(&script);
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("the wrapper script is stat-able")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("the wrapper script becomes executable");
    }
    // Off-unix the wrapper script is never run (the sessions are), so
    // there is nothing to chmod.
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arm vocabulary: baseline, every canonical mode (derived from
    /// the one table that owns the vocabulary — a new mode row extends
    /// the arm coverage automatically), aliases resolving to their
    /// canonical spelling, and the loud usage error for anything else.
    #[test]
    fn arms_resolve_through_the_mode_vocabulary() {
        assert_eq!(
            Arm::parse("baseline"),
            Ok(Arm {
                label: "baseline",
                mode: Mode::Jsonl
            })
        );
        assert_eq!(Arm::parse("BASELINE"), Arm::parse("baseline"));
        // The baseline is the jsonl pin: one mode under two labels, so
        // the duplicate guard catches `baseline,jsonl` the way it
        // catches `md,markdown`.
        assert_eq!(
            Arm::parse("baseline").unwrap().mode(),
            Arm::parse("jsonl").unwrap().mode()
        );
        for name in Mode::names_in_order() {
            assert_eq!(Arm::parse(name).unwrap().label(), name, "{name}");
        }
        // An alias carries the canonical spelling, so two spellings of
        // one mode are the same arm — the duplicate guard in main can
        // catch them.
        assert_eq!(Arm::parse("md"), Arm::parse("markdown"));
        assert_eq!(Arm::parse("md").unwrap().label(), "markdown");
        assert_eq!(Arm::parse("TOON").unwrap().label(), "toon");
        let error = Arm::parse("tabel").unwrap_err();
        assert!(
            error.contains("--mode value"),
            "the usage error names the vocabulary: {error}"
        );
    }

    /// The wrapper quotes the binary path for sh — a path carrying a
    /// shell metacharacter or a space must survive verbatim.
    #[test]
    fn the_wrapper_quotes_the_binary_path() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("my bin's $HOME tool");
            std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();
            make_executable(&bin);
            let arm_dir = dir.path().join("baseline");
            write_wrapper(&arm_dir, &Arm::parse("baseline").unwrap(), &bin);
            let script = std::fs::read_to_string(arm_dir.join("relate")).unwrap();
            assert!(
                script.contains(r"my bin'\''s $HOME tool'"),
                "the path is single-quoted with the sh escape: {script}"
            );
            // The generated wrapper actually runs: exec the quoted path.
            let status = std::process::Command::new(dir.path().join("baseline/relate"))
                .status()
                .unwrap();
            assert!(status.success(), "the wrapper execs the real binary");
        }
    }
}
