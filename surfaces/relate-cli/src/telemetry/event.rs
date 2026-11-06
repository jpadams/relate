//! Event taxonomy (ADR-0079 §4).
//!
//! Four events, all carrying the `RELATE-CLI_` app prefix:
//!
//! - `RELATE-CLI_STARTUP` — `kind`
//! - `RELATE-CLI_QUERY` — `success`, `source`, `storage`, `output_mode`
//! - `RELATE-CLI_COMMAND` — `command`, `action`, `success`
//! - `RELATE-CLI_HELP` — (base dimensions only)
//!
//! Base dimensions are added per event when the envelope is built (see
//! `envelope.rs`).
//!
//! **Hard privacy rule (ADR-0079 §4):** the event builder types have no
//! fields for query text, database paths, or parameter values, so capturing
//! those is a compile error, not a policy violation. Do not add such fields
//! without a new ADR referencing ADR-0079.

use serde_json::{Map, Value};

/// `RELATE-CLI_STARTUP` `kind` (ADR-0079 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupKind {
    Oneshot,
    Stdin,
    Repl,
    Version,
    Help,
}

impl StartupKind {
    fn as_str(self) -> &'static str {
        match self {
            StartupKind::Oneshot => "oneshot",
            StartupKind::Stdin => "stdin",
            StartupKind::Repl => "repl",
            StartupKind::Version => "version",
            StartupKind::Help => "help",
        }
    }
}

/// `RELATE-CLI_QUERY` `source` (ADR-0079 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuerySource {
    Oneshot,
    Stdin,
    Repl,
}

impl QuerySource {
    fn as_str(self) -> &'static str {
        match self {
            QuerySource::Oneshot => "oneshot",
            QuerySource::Stdin => "stdin",
            QuerySource::Repl => "repl",
        }
    }

    /// Batch (one-shot or piped stdin) vs interactive. Batch runs stop at
    /// the first failure and report progress (ADR-0199); an interactive line
    /// keeps going. Exhaustive, so a new variant fails to compile here until
    /// the batch policy classifies it.
    pub(crate) fn is_batch(self) -> bool {
        match self {
            QuerySource::Oneshot | QuerySource::Stdin => true,
            QuerySource::Repl => false,
        }
    }
}

/// The startup kind of a query run is its query source: the CLI computes the
/// invocation-mode trichotomy once as a `QuerySource` and derives the
/// `STARTUP` kind from it. `StartupKind::Version`/`Help` are emitted on the
/// post-parse early-exit paths and never flow through the query-run path, so
/// they are not part of this mapping.
impl From<QuerySource> for StartupKind {
    fn from(source: QuerySource) -> Self {
        match source {
            QuerySource::Oneshot => StartupKind::Oneshot,
            QuerySource::Stdin => StartupKind::Stdin,
            QuerySource::Repl => StartupKind::Repl,
        }
    }
}

/// `RELATE-CLI_QUERY` `storage` (ADR-0079 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    Memory,
    Persistent,
}

impl StorageKind {
    /// The storage classification of a database path/URI (ADR-0079 §4): the
    /// one mapping from `is_memory_uri`, so `main` and `lab` classify the
    /// same way.
    pub(crate) fn for_uri(db_path: &str) -> Self {
        if relate_storage::is_memory_uri(db_path) {
            StorageKind::Memory
        } else {
            StorageKind::Persistent
        }
    }

    /// The storage classification of a resolved store open (LABS-903,
    /// ADR-0500): a memory URI, or a mode that turns the store into the
    /// in-memory store (`OpenMode::Ephemeral`, the `--load` memory
    /// turn-into), is the in-memory store — whatever path or URL the
    /// content came from — and every other open is the path's own
    /// [`for_uri`](Self::for_uri) mapping. The mode-aware mapping lives
    /// here, beside the path mapping it extends, so no call site carries a
    /// mode conditional of its own.
    pub(crate) fn for_store(db_path: &str, mode: relate::OpenMode) -> Self {
        if mode == relate::OpenMode::Ephemeral {
            StorageKind::Memory
        } else {
            StorageKind::for_uri(db_path)
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            StorageKind::Memory => "memory",
            StorageKind::Persistent => "persistent",
        }
    }

    /// Whether this kind is a persistent store, for the exit/checkpoint
    /// boundary, which takes a bool.
    pub(crate) fn is_persistent(self) -> bool {
        matches!(self, StorageKind::Persistent)
    }
}

/// `RELATE-CLI_QUERY` `output_mode` — mirrors the formatter's modes;
/// the `From<&relate_formatter::Mode>` mapping lives below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Table,
    Csv,
    Json,
    Jsonl,
    Markdown,
    Cypher,
    Toon,
}

impl OutputMode {
    fn as_str(self) -> &'static str {
        match self {
            OutputMode::Table => "table",
            OutputMode::Csv => "csv",
            OutputMode::Json => "json",
            OutputMode::Jsonl => "jsonl",
            OutputMode::Markdown => "markdown",
            OutputMode::Cypher => "cypher",
            OutputMode::Toon => "toon",
        }
    }
}

/// Map the formatter's output mode onto the telemetry taxonomy (ADR-0079
/// §4). `OutputMode` mirrors `relate_formatter::Mode` variant-for-variant
/// except `Envelope`, which reports as `Json` (ADR-0195).
impl From<&relate_formatter::Mode> for OutputMode {
    fn from(mode: &relate_formatter::Mode) -> Self {
        match mode {
            relate_formatter::Mode::Table => OutputMode::Table,
            relate_formatter::Mode::Csv => OutputMode::Csv,
            relate_formatter::Mode::Json => OutputMode::Json,
            relate_formatter::Mode::Jsonl => OutputMode::Jsonl,
            // ADR-0195: the closed ADR-0079 value set does not grow; the
            // envelope is JSON output and reports as such, and so does the
            // speedscope evented-profile document.
            relate_formatter::Mode::Envelope => OutputMode::Json,
            relate_formatter::Mode::Speedscope => OutputMode::Json,
            relate_formatter::Mode::Markdown => OutputMode::Markdown,
            relate_formatter::Mode::Cypher => OutputMode::Cypher,
            relate_formatter::Mode::Toon => OutputMode::Toon,
        }
    }
}

/// One telemetry event. Variants carry exactly the properties listed in
/// ADR-0079 §4 — there are deliberately no fields for query text, database
/// paths, or parameter values.
#[derive(Debug, Clone)]
pub enum Event {
    /// `RELATE-CLI_STARTUP {kind}`.
    Startup {
        /// How the CLI was invoked.
        kind: StartupKind,
    },
    /// `RELATE-CLI_QUERY {success, source, storage, output_mode}`.
    Query {
        /// Whether the query succeeded.
        success: bool,
        /// Which invocation mode ran the query.
        source: QuerySource,
        /// In-memory vs. persistent database.
        storage: StorageKind,
        /// Result output format.
        output_mode: OutputMode,
    },
    /// `RELATE-CLI_COMMAND {command, action, success}`.
    Command {
        /// Lifecycle command name, e.g. `"skill"`, `"upgrade"`, `"telemetry"`.
        command: &'static str,
        /// Command action, e.g. `"install"`; `None` for action-less commands.
        action: Option<&'static str>,
        /// Whether the command succeeded.
        success: bool,
    },
    /// `RELATE-CLI_HELP {}` — fired when help is shown.
    Help,
}

impl Event {
    /// The Mixpanel event name, with the `RELATE-CLI_` app prefix.
    pub fn name(&self) -> &'static str {
        match self {
            Event::Startup { .. } => "RELATE-CLI_STARTUP",
            Event::Query { .. } => "RELATE-CLI_QUERY",
            Event::Command { .. } => "RELATE-CLI_COMMAND",
            Event::Help => "RELATE-CLI_HELP",
        }
    }

    /// Event-specific properties (base dimensions are added separately).
    pub(crate) fn properties(&self) -> Map<String, Value> {
        let mut m = Map::new();
        match self {
            Event::Startup { kind } => {
                m.insert("kind".into(), kind.as_str().into());
            }
            Event::Query {
                success,
                source,
                storage,
                output_mode,
            } => {
                m.insert("success".into(), (*success).into());
                m.insert("source".into(), source.as_str().into());
                m.insert("storage".into(), storage.as_str().into());
                m.insert("output_mode".into(), output_mode.as_str().into());
            }
            Event::Command {
                command,
                action,
                success,
            } => {
                m.insert("command".into(), (*command).into());
                if let Some(action) = action {
                    m.insert("action".into(), (*action).into());
                }
                m.insert("success".into(), (*success).into());
            }
            Event::Help => {}
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_carry_cli_prefix() {
        assert_eq!(
            Event::Startup {
                kind: StartupKind::Help
            }
            .name(),
            "RELATE-CLI_STARTUP"
        );
        assert_eq!(
            Event::Query {
                success: true,
                source: QuerySource::Oneshot,
                storage: StorageKind::Memory,
                output_mode: OutputMode::Csv,
            }
            .name(),
            "RELATE-CLI_QUERY"
        );
        assert_eq!(
            Event::Command {
                command: "upgrade",
                action: None,
                success: true,
            }
            .name(),
            "RELATE-CLI_COMMAND"
        );
        assert_eq!(Event::Help.name(), "RELATE-CLI_HELP");
    }
}
