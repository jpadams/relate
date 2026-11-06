//! `--fetch-params` fetch half (ADR-0267): fetch a URL (or read a file),
//! parse it as JSON, and bind it as the query parameter `NAME` — the same
//! JSON→`CypherValue` conversion `--param` uses. A CLI-process fetch, not an
//! engine capability: the engine's `LOAD CSV` policy (ADR-0081) is untouched.
//! `NAME=SOURCE` splitting and source classification live ungated in
//! `params.rs` (ADR-0270): every flag parses in every build.

use std::io::Read;
use std::time::Duration;

use relate_model::serde_value::try_json_to_param_value;
use relate_model::CypherValue;

use crate::params::FetchSource;
use crate::url_fetch::{get, FetchLimits};
use relate_wire::read_cap::{read_capped, CappedRead};

/// Cap and timeouts for `--fetch-params` sources (64 MiB, 60 s global) —
/// independently set from `pull`'s constants (ADR-0080).
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Read the source, parse it as JSON, and convert it to a `CypherValue`
/// bindable as a query parameter.
pub(crate) fn fetch_source(name: &str, source: &FetchSource) -> Result<CypherValue, String> {
    let prefix = format!("--fetch-params {name}: ");
    let bytes = match source {
        FetchSource::Url(url) => fetch_url(url),
        FetchSource::File(path) => read_file(path),
    }
    .map_err(|e| format!("{prefix}{e}"))?;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{prefix}invalid JSON: {e}"))?;
    try_json_to_param_value(&json).map_err(|e| format!("{prefix}{e}"))
}

/// `--fetch-params`'s wording over the shared GET's facts: the knobs are
/// this flag's ([`MAX_SOURCE_BYTES`]/[`READ_TIMEOUT`]), and the cap sentence
/// is the one per-caller part of the message ([`FetchFailure::message`]
/// states the rest).
fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    get(
        url,
        &FetchLimits {
            cap_bytes: MAX_SOURCE_BYTES as u64,
            timeout: READ_TIMEOUT,
            // A plain-`http` source anywhere is this flag's documented
            // contract (ADR-0267), so the redirect-downgrade pin stays off
            // here; the remote `--database` fetch sets it.
            https_only: false,
        },
    )
    .map_err(|f| f.message(&cap_sentence()))
}

/// The one cap sentence both read paths render: the URL fetch and the file
/// read share [`MAX_SOURCE_BYTES`], so they share its wording too.
fn cap_sentence() -> String {
    format!(
        "source exceeded the {} MiB cap",
        MAX_SOURCE_BYTES / (1024 * 1024)
    )
}

fn read_file(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    read_source(file).map_err(|e| format!("reading {}: {e}", path.display()))
}

/// Read a file source to end under [`MAX_SOURCE_BYTES`]: one byte past the
/// cap refuses the source instead of truncating it — the same guard and the
/// same wording the HTTP body read carries (and no stat-then-read gap). The
/// read is the wire kit's shared guard
/// ([`relate_wire::read_cap::read_capped`]); only
/// the cap and the wording are `--fetch-params`'s. `relate-load` keeps
/// its own 64 MiB cap and wording because `fetch-params` deliberately
/// depends only on `ureq`, not on `relate-load` (ADR-0267).
fn read_source(reader: impl Read) -> Result<Vec<u8>, String> {
    match read_capped(reader, MAX_SOURCE_BYTES as u64) {
        Ok(CappedRead::Complete(bytes)) => Ok(bytes),
        Ok(CappedRead::Exceeded) => Err(cap_sentence()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_source_detects_and_rejects_oversized_sources() {
        use std::io::Cursor;
        let cap = MAX_SOURCE_BYTES;
        // exactly at the cap: read through
        let at_cap = read_source(Cursor::new(vec![0u8; cap])).unwrap();
        assert_eq!(at_cap.len(), cap);
        // one byte over: detect, never truncate
        let err = read_source(Cursor::new(vec![0u8; cap + 1])).unwrap_err();
        assert_eq!(err, "source exceeded the 64 MiB cap");
    }

    #[test]
    fn file_load_parses_json_to_param_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.json");
        std::fs::write(&path, r#"{"items": [1, 2]}"#).unwrap();
        let value = fetch_source("v", &FetchSource::File(path)).unwrap();
        let s = format!("{value:?}");
        assert!(s.contains("items"), "got: {s}");
    }
}
