//! The extension host (ADR-0429 §5, ADR-0430 §6–§7): the layer the `run`
//! verb dispatches into — the host that parses `run <name>` argv from
//! the manifest's flag table and drives the child's admitted batches
//! against a host-owned store.
//!
//! Because the grammar is host-side, every CLI convention reaches
//! extensions for free (ADR-0429 §5): the action's help text renders from
//! the table ([`action_help`]), did-you-mean suggests from the same flag
//! list the parser accepts ([`flag_candidates`]), usage refusals are the
//! parser's [`ParseError`]s (exit 2 at the verb's usage-error path,
//! ADR-0430 §5's flag class), and the store flag is host property: the
//! host resolves `-d`/`--database` itself (ADR-0383) — the value never
//! reaches the child's envelope (ADR-0430 §6) — while `--db` stays
//! rejected with the removed-spelling wording, no near-miss hint.
//!
//! Execution reuses the `run_import` lifecycle unchanged (ADR-0430 §7):
//! the `:memory:` default (an explicit store target exits after the
//! summary; an omitted one opens the shell on a TTY, ADR-0286), the child's
//! writing calls framed by the shared write layer under the manifest's
//! [`CommitPolicy`](crate::extensions::manifest::CommitPolicy), the summary
//! printed through `print_error`-grade sanitization, the final checkpoint.
//! The child is driven as an MCP client by the mount's stdio server
//! (`extensions::mcp`, ADR-0442/ADR-0447) under that store, fresh-store
//! check, commit policy, summary rendering, handoff and checkpoint. Every
//! runtime failure is a [`HostError`] — typed variants with pinned texts,
//! the `ImportError` pattern — rendered as one `Error:` line, exit 1.

mod help;
mod parse;
mod run;

pub(crate) use help::{action_help, action_usage};
pub(crate) use parse::{
    check_manifest, default_envelope, flag_candidates, parse_action_args, ExtensionRun,
};
pub(crate) use run::{run_action, HostError, RunSummary};

#[cfg(test)]
mod tests;
