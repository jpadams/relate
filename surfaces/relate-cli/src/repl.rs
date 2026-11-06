//! Interactive TTY prompt loop (ADR-0073).
//!
//! On a terminal, rustyline provides GNU readline-style editing (arrow keys,
//! Home/End, emacs bindings) and in-session history. Builds without the
//! `repl` feature (ADR-0104) use the plain-stdin loop instead. Piped stdin
//! and one-shot queries never enter this module.

use std::io::{self, BufRead, Write};

use relate_splitter::input_is_complete;

/// Primary prompt when the multi-line buffer is empty.
pub const PRIMARY_PROMPT: &str = "cypher> ";
/// Continuation prompt while `input_is_complete` is false.
pub const CONTINUATION_PROMPT: &str = "   ...> ";

/// Outcome of feeding one editor / stdin event into the pending buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplStep {
    /// Keep reading (incomplete Cypher, an empty submit, or Ctrl-C).
    Continue,
    /// Buffer is a complete statement / dot-command; execute it.
    Submit(String),
    /// EOF. Execute `pending` if it is `Some`, then exit.
    Exit { pending: Option<String> },
}

/// Prompt shown for the current pending buffer.
pub fn repl_prompt(pending: &str) -> &'static str {
    if pending.trim().is_empty() {
        PRIMARY_PROMPT
    } else {
        CONTINUATION_PROMPT
    }
}

/// Append one physical line (with or without a trailing newline) and decide
/// whether the buffer is ready to execute.
pub fn apply_line(buf: &mut String, line: &str) -> ReplStep {
    buf.push_str(line);
    if !line.ends_with('\n') {
        buf.push('\n');
    }
    if input_is_complete(buf) {
        ReplStep::Submit(std::mem::take(buf))
    } else {
        ReplStep::Continue
    }
}

/// Ctrl-C: drop a multi-line draft and return to `cypher>`.
#[cfg_attr(not(feature = "repl"), allow(dead_code))]
pub fn apply_interrupt(buf: &mut String) -> ReplStep {
    buf.clear();
    ReplStep::Continue
}

/// Ctrl-D / EOF: exit, executing any non-empty pending buffer first.
pub fn apply_eof(buf: &mut String) -> ReplStep {
    let pending = if buf.trim().is_empty() {
        None
    } else {
        Some(std::mem::take(buf))
    };
    ReplStep::Exit { pending }
}

/// History records completed, non-empty input as a single entry.
#[cfg_attr(not(feature = "repl"), allow(dead_code))]
pub fn history_entry(submitted: &str) -> Option<&str> {
    let trimmed = submitted.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Run the TTY prompt loop. `exec` is invoked for each complete submit
/// (including a non-empty pending buffer at EOF). Returns the process exit
/// code: 0 on clean EOF, 1 if the editor fails after it has already started.
/// Without the `repl` feature this is the plain-stdin loop.
pub fn run_interactive<F>(mut exec: F) -> i32
where
    F: FnMut(&str),
{
    #[cfg(feature = "repl")]
    match editor::run(&mut exec) {
        Ok(code) => return code,
        Err(e) => crate::print_error(format!(
            "warning: line editor unavailable ({e}); falling back to plain stdin"
        )),
    }
    run_plain_interactive(&mut exec)
}

/// The rustyline line editor (ADR-0073), compiled only with `repl`.
#[cfg(feature = "repl")]
mod editor {
    use rustyline::error::ReadlineError;
    use rustyline::{Config, DefaultEditor};

    use super::{apply_interrupt, apply_line, finish_eof, history_entry, repl_prompt, ReplStep};

    pub(super) fn new_editor() -> rustyline::Result<DefaultEditor> {
        let config = Config::builder().auto_add_history(false).build();
        DefaultEditor::with_config(config)
    }

    pub(super) fn run<F>(exec: &mut F) -> rustyline::Result<i32>
    where
        F: FnMut(&str),
    {
        let mut editor = new_editor()?;
        let mut buf = String::new();
        loop {
            match editor.readline(repl_prompt(&buf)) {
                Ok(line) => match apply_line(&mut buf, &line) {
                    ReplStep::Continue => {}
                    ReplStep::Submit(input) => {
                        if let Some(entry) = history_entry(&input) {
                            let _ = editor.add_history_entry(entry);
                        }
                        exec(&input);
                    }
                    ReplStep::Exit { .. } => unreachable!("apply_line never exits"),
                },
                Err(ReadlineError::Interrupted) => {
                    let _ = apply_interrupt(&mut buf);
                }
                Err(ReadlineError::Eof) => {
                    return Ok(finish_eof(exec, &mut buf));
                }
                Err(err) => return Err(err),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn editor_constructs_without_a_tty() {
            let _editor = super::new_editor()
                .expect("rustyline Editor::with_config should not require a TTY");
        }
    }
}

fn run_plain_interactive<F>(exec: &mut F) -> i32
where
    F: FnMut(&str),
{
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut buf = String::new();
    loop {
        print!("{}", repl_prompt(&buf));
        let _ = stdout.flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            return finish_eof(exec, &mut buf);
        }
        match apply_line(&mut buf, &line) {
            ReplStep::Continue => {}
            ReplStep::Submit(input) => exec(&input),
            ReplStep::Exit { .. } => unreachable!("apply_line never exits"),
        }
    }
}

fn finish_eof<F>(exec: &mut F, buf: &mut String) -> i32
where
    F: FnMut(&str),
{
    if let ReplStep::Exit { pending: Some(p) } = apply_eof(buf) {
        exec(&p);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_is_primary_when_buffer_is_blank() {
        assert_eq!(repl_prompt(""), PRIMARY_PROMPT);
        assert_eq!(repl_prompt("   \n"), PRIMARY_PROMPT);
    }

    #[test]
    fn prompt_is_continuation_while_cypher_is_open() {
        assert_eq!(repl_prompt("MATCH (n)\n"), CONTINUATION_PROMPT);
    }

    #[test]
    fn apply_line_submits_dot_command_on_newline() {
        let mut buf = String::new();
        assert_eq!(
            apply_line(&mut buf, ".help"),
            ReplStep::Submit(".help\n".to_string())
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn apply_line_keeps_unterminated_cypher() {
        let mut buf = String::new();
        assert_eq!(apply_line(&mut buf, "MATCH (n)"), ReplStep::Continue);
        assert_eq!(buf, "MATCH (n)\n");
        assert_eq!(
            apply_line(&mut buf, "RETURN n;"),
            ReplStep::Submit("MATCH (n)\nRETURN n;\n".to_string())
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn apply_line_accepts_read_line_newlines() {
        let mut buf = String::new();
        assert_eq!(
            apply_line(&mut buf, "RETURN 1;\n"),
            ReplStep::Submit("RETURN 1;\n".to_string())
        );
    }

    #[test]
    fn blank_line_at_primary_prompt_is_an_empty_submit() {
        let mut buf = String::new();
        match apply_line(&mut buf, "\n") {
            ReplStep::Submit(s) => assert!(s.trim().is_empty()),
            other => panic!("expected empty submit, got {other:?}"),
        }
    }

    #[test]
    fn interrupt_clears_pending_multiline() {
        let mut buf = String::from("MATCH (n)\n");
        assert_eq!(apply_interrupt(&mut buf), ReplStep::Continue);
        assert!(buf.is_empty());
        assert_eq!(repl_prompt(&buf), PRIMARY_PROMPT);
    }

    #[test]
    fn eof_on_empty_buffer_exits_without_pending() {
        let mut buf = String::new();
        assert_eq!(apply_eof(&mut buf), ReplStep::Exit { pending: None });
    }

    #[test]
    fn eof_on_pending_buffer_returns_it() {
        let mut buf = String::from("RETURN 1");
        assert_eq!(
            apply_eof(&mut buf),
            ReplStep::Exit {
                pending: Some("RETURN 1".to_string())
            }
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn history_skips_blank_submits() {
        assert_eq!(history_entry("\n"), None);
        assert_eq!(history_entry("   \n"), None);
        assert_eq!(history_entry("RETURN 1;\n"), Some("RETURN 1;"));
        assert_eq!(
            history_entry("MATCH (n)\nRETURN n;\n"),
            Some("MATCH (n)\nRETURN n;")
        );
    }
}
