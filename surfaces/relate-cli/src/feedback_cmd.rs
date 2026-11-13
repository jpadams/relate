//! `relate feedback submit|enable|disable|status` (ADR-0588 §3): the
//! person-consented feedback verbs, following the ADR-0069 lifecycle-command
//! grammar (first positional only; `./feedback` and `-- feedback` stay db
//! paths), plus the hidden `serve` MCP stdio path (§4).
//!
//! The crate owns the report model, its validation and redaction policy, the
//! consent record, the submit pipeline, the Mixpanel sender and the MCP
//! message loop. This module owns the grammar, the flag values, the
//! sender-owned report context and the process exit code — and, for `serve`,
//! the process stdio wiring.

use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use relate_feedback::authorization::{Authorization, FeedbackConsent};
use relate_feedback::consent;
use relate_feedback::report::{Context, Input, Invoker};
use relate_feedback::sender::{self, FeedbackSender};
use relate_feedback::serve::{self as mcp, Server};
use relate_feedback::state;
use relate_feedback::submit::{self, Request};

// The run's environment lookup: unset and empty both read as absent, the
// config-dir module's one rule for it.
use crate::config_dir::env_var as env;
use crate::parse_error::{flag_value, set_once, ParseError};

/// The command word, also the value every report's `command_id` filter
/// excludes: a report never names the feedback command itself.
const WORD: &str = "feedback";

/// What `relate feedback` should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackAction {
    /// Serve the MCP stdio server until the client closes stdin.
    Serve,
    /// Report one friction point: the report's caller-supplied fields.
    Submit(Input),
    /// Grant the feedback authorization for the default window.
    Enable,
    /// Revoke the feedback authorization.
    Disable,
    /// Print the effective feedback state.
    Status,
}

impl FeedbackAction {
    /// The CLI spelling of the action (also the telemetry event value).
    pub fn as_str(&self) -> &'static str {
        match self {
            FeedbackAction::Serve => "serve",
            FeedbackAction::Submit(_) => "submit",
            FeedbackAction::Enable => "enable",
            FeedbackAction::Disable => "disable",
            FeedbackAction::Status => "status",
        }
    }
}

/// Parse the argument tail after the `feedback` verb. Pure — returns the
/// usage-error message instead of exiting, so the grammar is unit-testable;
/// `main` turns `Err` into the hand-rolled parser's exit-2 convention.
pub fn parse_action(rest: &[String]) -> Result<FeedbackAction, ParseError> {
    let Some(action) = rest.first() else {
        return Err("feedback requires an action (submit, enable, disable, or status)".into());
    };
    match action.as_str() {
        "submit" => parse_submit(&rest[1..]).map(FeedbackAction::Submit),
        // Hidden: not offered by the help page, parsed like any other action.
        "serve" => no_more(rest, FeedbackAction::Serve),
        "enable" => no_more(rest, FeedbackAction::Enable),
        "disable" => no_more(rest, FeedbackAction::Disable),
        "status" => no_more(rest, FeedbackAction::Status),
        other => Err(ParseError::unexpected(WORD, other)),
    }
}

/// A flagless action: anything after the action word is an extra positional.
fn no_more(rest: &[String], action: FeedbackAction) -> Result<FeedbackAction, ParseError> {
    match rest.get(1) {
        Some(extra) => Err(ParseError::unexpected(WORD, extra)),
        None => Ok(action),
    }
}

fn parse_submit(rest: &[String]) -> Result<Input, ParseError> {
    let mut category = None;
    let mut expected = None;
    let mut observed = None;
    let mut command = None;
    let mut error_code = None;
    let mut model = None;
    let mut request_id = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--category" => set_once(
                WORD,
                &mut category,
                "--category",
                flag_value(&mut args, "--category", "a category")?.clone(),
            )?,
            "--expected" => set_once(
                WORD,
                &mut expected,
                "--expected",
                flag_value(&mut args, "--expected", "text")?.clone(),
            )?,
            "--observed" => set_once(
                WORD,
                &mut observed,
                "--observed",
                flag_value(&mut args, "--observed", "text")?.clone(),
            )?,
            "--command" => set_once(
                WORD,
                &mut command,
                "--command",
                flag_value(&mut args, "--command", "a command path")?.clone(),
            )?,
            "--error-code" => set_once(
                WORD,
                &mut error_code,
                "--error-code",
                flag_value(&mut args, "--error-code", "a code name")?.clone(),
            )?,
            "--model" => set_once(
                WORD,
                &mut model,
                "--model",
                flag_value(&mut args, "--model", "a model label")?.clone(),
            )?,
            "--request-id" => set_once(
                WORD,
                &mut request_id,
                "--request-id",
                flag_value(&mut args, "--request-id", "a UUID")?.clone(),
            )?,
            other => return Err(ParseError::unexpected(WORD, other)),
        }
    }
    Ok(Input {
        category: category.ok_or("feedback submit requires --category")?,
        expected: expected.ok_or("feedback submit requires --expected")?,
        observed: observed.ok_or("feedback submit requires --observed")?,
        command,
        error_code,
        agent_model: model,
        request_id,
    })
}

/// Run the action. Returns the process exit code; the caller (main) owns
/// `std::process::exit`. Every action needs the config directory, so it is
/// resolved once here rather than at each verb's head.
pub fn run(action: FeedbackAction) -> i32 {
    let dir = match config_dir(action.as_str()) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    match action {
        FeedbackAction::Serve => serve(&dir),
        FeedbackAction::Submit(input) => submit_report(&input, &dir),
        FeedbackAction::Enable => enable(&dir),
        FeedbackAction::Disable => disable(&dir),
        FeedbackAction::Status => status(&dir),
    }
}

/// The config directory every feedback state file lives in, or the
/// unresolved-base error as exit 1 (the `telemetry` command's contract).
fn config_dir(verb: &str) -> Result<PathBuf, i32> {
    crate::config_dir::config_dir()
        .ok_or_else(|| crate::config_dir::no_base(&format!("feedback {verb}")))
}

/// `feedback serve` (ADR-0588 §4): speak newline-delimited JSON-RPC 2.0 on
/// this process's stdio until the client closes stdin.
fn serve(dir: &Path) -> i32 {
    let sender = FeedbackSender::from_env(&env);
    let server = Server {
        dir,
        // The client launched a pipe, never a terminal.
        context: report_context(&env, pipe_invoker(&env)),
        env: &env,
        sender: &sender,
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    if let Err(error) = mcp::serve(&server, stdin.lock(), stdout.lock()) {
        crate::print_error(format!("Error: feedback serve: {error}"));
        return 1;
    }
    0
}

/// `feedback submit`: build the report's sender-owned context and run the
/// crate's pipeline. Prints the outcome document as one compact JSON line;
/// exit 0 for `sent` and `skipped` (a normal local decision), 1 otherwise.
fn submit_report(input: &Input, dir: &Path) -> i32 {
    let sender = FeedbackSender::from_env(&env);
    let context = report_context(&env, invoker(&env));
    let doc = submit::submit(
        &Request {
            dir,
            input,
            context: &context,
            env: &env,
            config_telemetry: state::load_config_telemetry(dir),
            now: Utc::now(),
        },
        &sender,
    );
    println!(
        "{}",
        serde_json::to_string(&doc).expect("an outcome document is plain JSON")
    );
    if doc.outcome().is_success() {
        0
    } else {
        1
    }
}

/// `feedback enable`: record the authorization with the destination the
/// report would use. A refused `RELATE_FEEDBACK_ENDPOINT`, or no destination
/// at all, is reported and records nothing: an authorization with no
/// destination would only turn every later submit into `not_configured`.
fn enable(dir: &Path) -> i32 {
    let endpoint = match sender::resolve_feedback_endpoint(&env) {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => {
            return refuse_enable(
                "no destination is configured: set RELATE_FEEDBACK_TOKEN or \
                 RELATE_FEEDBACK_ENDPOINT",
            )
        }
        Err(url) => {
            return refuse_enable(&format!(
                "RELATE_FEEDBACK_ENDPOINT={url}: https on a Mixpanel host or http on \
                 loopback hosts is required"
            ))
        }
    };
    let mut record = FeedbackConsent::load(dir);
    let until = record.authorize(Utc::now(), endpoint);
    if let Err(code) = save(&record, dir, "enable") {
        return code;
    }
    println!(
        "feedback enabled until {} ({})",
        until.to_rfc3339_opts(SecondsFormat::Secs, true),
        state::config_path(dir).display()
    );
    0
}

/// The one `feedback enable` refusal: nothing is recorded, exit 1.
fn refuse_enable(detail: &str) -> i32 {
    crate::print_error(format!("Error: feedback enable: refusing {detail}"));
    1
}

/// Persist the consent record, reporting a write failure as
/// `Error: feedback {verb}: writing {path}` with exit 1 — one shape for both
/// verbs, each keeping only its success line.
fn save(record: &FeedbackConsent, dir: &Path, verb: &str) -> Result<(), i32> {
    record.save(dir).map_err(|e| {
        crate::print_error(format!(
            "Error: feedback {verb}: writing {}: {e}",
            state::config_path(dir).display()
        ));
        1
    })
}

/// `feedback disable`: clear the consent record, which revokes the
/// authorization.
fn disable(dir: &Path) -> i32 {
    if let Err(code) = save(&FeedbackConsent::default(), dir, "disable") {
        return code;
    }
    println!("feedback disabled ({})", state::config_path(dir).display());
    0
}

/// `feedback status`: the effective state plus what decided it — the
/// telemetry opt-out layer first (it suppresses every send), then the
/// authorization verdict.
fn status(dir: &Path) -> i32 {
    let state = consent::resolve(&env, state::load_config_telemetry(dir));
    println!(
        "{}",
        status_line(state, &FeedbackConsent::load(dir), Utc::now())
    );
    0
}

/// The `status` output line, e.g. `enabled (until 2026-12-12T00:00:00Z)` or
/// `disabled (not enabled)`.
fn status_line(
    state: consent::EffectiveState,
    record: &FeedbackConsent,
    now: DateTime<Utc>,
) -> String {
    if !state.enabled() {
        return state.status_line();
    }
    match record.authorization(now) {
        Authorization::Authorized { until, .. } => format!(
            "enabled (until {})",
            until.to_rfc3339_opts(SecondsFormat::Secs, true)
        ),
        Authorization::Expired => "disabled (authorization expired)".to_string(),
        Authorization::Missing => "disabled (not enabled)".to_string(),
    }
}

/// The sender-owned report context, shared by the serve and shell paths so
/// the same agent reports the same shape either way. Both
/// environment-derived fields read through the caller's one lookup, so a
/// caller can pin the record's environment. The harness label is the raw
/// `AI_AGENT` value: the crate's policy is the single gate on what may be
/// sent, so nothing is dropped here twice.
fn report_context(env: &dyn Fn(&str) -> Option<String>, invoker: Invoker) -> Context {
    Context {
        cli_version: crate::VERSION.to_string(),
        os: os_label(),
        os_arch: arch_label(),
        invoker,
        agent_harness: env("AI_AGENT"),
        commands: visible_commands(),
        error_codes: crate::agent_context_cmd::error_code_names(),
    }
}

/// The report's `invoker` classification: the shared launch classifier,
/// spelled in the report's vocabulary (`ci` reads `script`).
pub(crate) fn invoker(env: &dyn Fn(&str) -> Option<String>) -> Invoker {
    match crate::agent_mode::launched_by(env) {
        crate::agent_mode::Launched::Agent => Invoker::Agent,
        crate::agent_mode::Launched::Ci => Invoker::Script,
        crate::agent_mode::Launched::Human => Invoker::Human,
    }
}

/// The `serve` path's `invoker`: an MCP client speaks over a pipe, never a
/// terminal, so the human classification folds into script — the same
/// classifier, one arm narrowed.
fn pipe_invoker(env: &dyn Fn(&str) -> Option<String>) -> Invoker {
    match invoker(env) {
        Invoker::Human => Invoker::Script,
        other => other,
    }
}

/// The visible command paths a report may name (`--command`): the compiled-in
/// command words, feedback itself excluded (a report never names the feedback
/// command).
fn visible_commands() -> Vec<String> {
    crate::help::commands()
        .iter()
        .filter(|spec| spec.word != WORD)
        .map(|spec| spec.word.to_string())
        .collect()
}

/// The report's `os` property: `linux`, `darwin` or `windows`; any other
/// platform is omitted rather than guessed.
fn os_label() -> Option<String> {
    match std::env::consts::OS {
        "linux" => Some("linux".to_string()),
        "macos" => Some("darwin".to_string()),
        "windows" => Some("windows".to_string()),
        _ => None,
    }
}

/// The report's `os_arch` property: `arm64` or `amd64`; any other
/// architecture is omitted rather than guessed.
fn arch_label() -> Option<String> {
    match std::env::consts::ARCH {
        "aarch64" => Some("arm64".to_string()),
        "x86_64" => Some("amd64".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::env_of;

    fn s(value: &str) -> String {
        value.to_string()
    }

    fn args(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| s(x)).collect()
    }

    fn authorized(until: &str) -> FeedbackConsent {
        FeedbackConsent {
            endpoint: Some("https://api.mixpanel.com/track".to_string()),
            authorized_until: Some(until.to_string()),
        }
    }

    #[test]
    fn parse_all_actions() {
        assert_eq!(parse_action(&args(&["serve"])), Ok(FeedbackAction::Serve));
        assert_eq!(parse_action(&args(&["enable"])), Ok(FeedbackAction::Enable));
        assert_eq!(
            parse_action(&args(&["disable"])),
            Ok(FeedbackAction::Disable)
        );
        assert_eq!(parse_action(&args(&["status"])), Ok(FeedbackAction::Status));
        assert_eq!(FeedbackAction::Enable.as_str(), "enable");
        assert_eq!(FeedbackAction::Serve.as_str(), "serve");
    }

    #[test]
    fn parse_bare_verb_requires_an_action() {
        assert_eq!(
            parse_action(&[]),
            Err("feedback requires an action (submit, enable, disable, or status)".into())
        );
    }

    #[test]
    fn parse_unknown_action_and_extra_arguments_are_unexpected() {
        assert_eq!(
            parse_action(&args(&["frobnicate"])),
            Err(ParseError::unexpected("feedback", "frobnicate"))
        );
        for action in ["serve", "status"] {
            assert_eq!(
                parse_action(&args(&[action, "extra"])),
                Err(ParseError::unexpected("feedback", "extra"))
            );
        }
    }

    #[test]
    fn parse_submit_requires_its_three_fields() {
        for (rest, missing) in [
            (args(&["submit"]), "--category"),
            (
                args(&["submit", "--category", "unclear_error"]),
                "--expected",
            ),
            (
                args(&["submit", "--category", "unclear_error", "--expected", "x"]),
                "--observed",
            ),
        ] {
            let err = parse_action(&rest).unwrap_err().to_string();
            assert!(err.contains(missing), "{rest:?}: {err}");
        }
    }

    #[test]
    fn parse_submit_accepts_every_flag_in_any_order() {
        let parsed = parse_action(&args(&[
            "submit",
            "--observed",
            "it printed help",
            "--category",
            "unclear_error",
            "--expected",
            "the info command to print the version",
            "--command",
            "info",
            "--error-code",
            "usage_error",
            "--model",
            "claude-opus-5-5",
            "--request-id",
            "123e4567-e89b-42d3-a456-426614174000",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            FeedbackAction::Submit(Input {
                category: s("unclear_error"),
                expected: s("the info command to print the version"),
                observed: s("it printed help"),
                command: Some(s("info")),
                error_code: Some(s("usage_error")),
                agent_model: Some(s("claude-opus-5-5")),
                request_id: Some(s("123e4567-e89b-42d3-a456-426614174000")),
            })
        );
    }

    #[test]
    fn parse_submit_rejects_unknown_and_duplicate_flags() {
        assert_eq!(
            parse_action(&args(&["submit", "--bogus", "x"])),
            Err(ParseError::unexpected("feedback", "--bogus"))
        );
        let duplicated = args(&[
            "submit",
            "--category",
            "unclear_error",
            "--category",
            "other_friction",
            "--expected",
            "x",
            "--observed",
            "y",
        ]);
        assert!(parse_action(&duplicated)
            .unwrap_err()
            .to_string()
            .contains("only once"));
    }

    #[test]
    fn status_line_reports_the_deciding_layer_and_the_authorization_window() {
        let now = DateTime::parse_from_rfc3339("2026-11-12T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let enabled = consent::EffectiveState::Default;
        assert_eq!(
            status_line(enabled, &authorized("2026-12-12T12:00:00Z"), now),
            "enabled (until 2026-12-12T12:00:00Z)"
        );
        assert_eq!(
            status_line(enabled, &FeedbackConsent::default(), now),
            "disabled (not enabled)"
        );
        assert_eq!(
            status_line(enabled, &authorized("2026-11-01T12:00:00Z"), now),
            "disabled (authorization expired)"
        );
        for disabled in [
            consent::EffectiveState::DoNotTrack,
            consent::EffectiveState::Env(false),
            consent::EffectiveState::ConfigFile(false),
        ] {
            assert_eq!(
                status_line(disabled, &authorized("2026-12-12T12:00:00Z"), now),
                disabled.status_line()
            );
        }
    }

    #[test]
    fn invoker_classification() {
        assert_eq!(invoker(&env_of(&[])), Invoker::Human);
        assert_eq!(invoker(&env_of(&[("CI", "true")])), Invoker::Script);
        assert_eq!(invoker(&env_of(&[("CI", "0")])), Invoker::Human);
        assert_eq!(invoker(&env_of(&[("CLAUDECODE", "1")])), Invoker::Agent);
        // A served process has no terminal behind it.
        assert_eq!(pipe_invoker(&env_of(&[])), Invoker::Script);
        assert_eq!(pipe_invoker(&env_of(&[("CI", "true")])), Invoker::Script);
        assert_eq!(
            pipe_invoker(&env_of(&[("CLAUDECODE", "1")])),
            Invoker::Agent
        );
    }

    #[test]
    fn report_context_reads_the_supplied_environment() {
        let context = report_context(&env_of(&[("AI_AGENT", "claude-code")]), Invoker::Agent);
        assert_eq!(context.agent_harness.as_deref(), Some("claude-code"));
        assert_eq!(context.invoker, Invoker::Agent);
        assert_eq!(context.cli_version, crate::VERSION);
        assert_eq!(
            report_context(&env_of(&[]), Invoker::Human).agent_harness,
            None
        );
        // The harness label crosses the trust boundary in the crate's policy,
        // not here: the raw value is carried and `keep_or_drop` decides.
        assert_eq!(
            report_context(&env_of(&[("AI_AGENT", "Claude Code")]), Invoker::Agent).agent_harness,
            Some("Claude Code".to_string())
        );
    }

    #[test]
    fn the_error_code_vocabulary_is_the_exit_table() {
        let names = crate::agent_context_cmd::error_code_names();
        assert!(names.contains(&s("timeout")), "ADR-0241 filled code 7");
        assert!(!names.contains(&s("rate_limited")), "no such code");
        assert!(
            names.iter().all(|name| name != "success"),
            "exit 0 is not an error code"
        );
    }

    #[test]
    fn visible_commands_exclude_feedback() {
        let commands = visible_commands();
        assert!(commands.iter().any(|word| word == "query"));
        assert!(commands.iter().all(|word| word != WORD));
    }

    #[test]
    fn os_and_arch_labels_are_the_report_vocabulary() {
        // The report's two optional sender fields, on this host.
        for label in [os_label(), arch_label()].into_iter().flatten() {
            assert!(
                ["linux", "darwin", "windows", "arm64", "amd64"].contains(&label.as_str()),
                "{label}"
            );
        }
    }
}
