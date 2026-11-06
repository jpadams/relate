//! Rows for `db.stats.retrieve` (ADR-0497 §6): the one added procedure the
//! Cypher-25 corpus calls. The GRAPH COUNTS section is derived from the live
//! catalogs (its sibling [`graph_counts`]); any other section name — and a
//! NULL section, which the CALL type check admits — is refused with a typed
//! error rather than answered with an empty or fabricated map, and never
//! with a panic.

use std::collections::BTreeMap;

use relate_core::{Error, Result, Value};

use crate::overlay::ExecGraph;

/// The one section the engine serves.
const GRAPH_COUNTS: &str = "GRAPH COUNTS";

/// Rows for `db.stats.retrieve`, one row for the requested section. The
/// producer only ever runs under a call's bound arguments; `section` is
/// `None` when the bound value is NULL (the procedure type check admits null
/// for a required STRING), which is a typed rejection, never a panic.
pub(super) fn stats_retrieve_rows(
    graph: &ExecGraph<'_>,
    section: Option<&str>,
) -> Result<Vec<BTreeMap<String, Value>>> {
    match section {
        Some(GRAPH_COUNTS) => Ok(vec![super::graph_counts::graph_counts_row(
            graph,
            GRAPH_COUNTS,
        )?]),
        Some(section) => Err(Error::ty(format!(
            "InvalidArgumentValue: db.stats.retrieve cannot serve section '{section}'"
        ))),
        None => Err(Error::ty(
            "InvalidArgumentValue: db.stats.retrieve expects a STRING section, got NULL",
        )),
    }
}
