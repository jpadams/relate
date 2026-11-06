//! One capped HTTP GET for every CLI-process fetch (ADR-0267's posture):
//! the `--fetch-params` source and the remote `--database` bundle (LABS-872)
//! fetch through here — ureq, one global timeout, non-2xx surfaced as a
//! clean status fact, and the body read under the caller's cap through the
//! wire kit's `read_capped`/`read_capped_into` guards, buffered or streamed
//! (one byte past the cap refuses the source
//! instead of truncating it). Each caller keeps its own cap, timeout, and
//! error wording: the knobs are deliberately independent (ADR-0267's
//! precedent for `pull`'s constants), so [`FetchFailure`] reports the fact
//! and the caller names it.
//!
//! A CLI-process fetch, not an engine capability: the engine's `LOAD CSV`
//! policy (ADR-0081) is untouched. No URL allowlist — the same trust as a
//! caller-typed path or pipe.

use std::io::{Read, Write};
use std::time::Duration;

use relate_wire::read_cap::{read_capped, read_capped_into, CappedCopy, CappedRead};

/// The knobs one capped GET carries: the body cap, the global timeout, and
/// the transport pin.
pub(crate) struct FetchLimits {
    pub(crate) cap_bytes: u64,
    pub(crate) timeout: Duration,
    /// Pin **every hop** of the redirect chain to https — the LABS-470
    /// integrity pin the release channel's client carries
    /// (`release_channel::http::http_agent`). A caller whose own scheme
    /// policy is https-only sets it, so a `302` cannot downgrade the
    /// transport past the policy the URL already passed. `false` leaves
    /// ureq's default (a plain-`http` source that is itself the documented
    /// contract, as `--fetch-params` sources are).
    pub(crate) https_only: bool,
}

/// Why a GET failed — the fact each caller words for its own flag.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FetchFailure {
    /// A non-success HTTP status.
    Status(u16),
    /// The body exceeded the caller's cap.
    ExceededCap,
    /// The request itself failed (transport, DNS, timeout).
    Transport(String),
    /// Reading the response body failed mid-stream.
    Read(String),
    /// Writing the body to the caller's sink failed (the streaming form).
    Write(String),
}

impl FetchFailure {
    /// The failure as the caller's message. Four arms are one vocabulary
    /// every fetch shares; only the cap's wording is per-caller (each flag
    /// states its own cap), so the caller supplies that one sentence.
    pub(crate) fn message(&self, cap_sentence: &str) -> String {
        match self {
            Self::Status(status) => format!("HTTP {status}"),
            Self::ExceededCap => cap_sentence.to_string(),
            Self::Transport(e) => format!("request failed: {e}"),
            Self::Read(e) => format!("reading response: {e}"),
            Self::Write(e) => format!("writing the response: {e}"),
        }
    }
}

/// The one request-and-status prologue every fetch here rides (the same
/// posture as `pull`'s HTTP path, ADR-0080: one global timeout, non-2xx
/// surfaced as a clean status fact): build the pinned client, issue the
/// GET, and hand back the body reader the caller reads its own way —
/// buffered ([`get`]) or streamed ([`get_into`]).
fn open(url: &str, limits: &FetchLimits) -> Result<impl Read, FetchFailure> {
    let response = client(limits)
        .get(url)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| FetchFailure::Transport(e.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(FetchFailure::Status(status.as_u16()));
    }
    Ok(response.into_body().into_reader())
}

/// GET `url` and read its whole body under `limits.cap_bytes`. The request
/// carries `limits.timeout` as its one global bound and `limits.https_only`
/// as its transport pin; a non-2xx status is [`FetchFailure::Status`] (the
/// caller words it, e.g. `HTTP 404`).
pub(crate) fn get(url: &str, limits: &FetchLimits) -> Result<Vec<u8>, FetchFailure> {
    let body = open(url, limits)?;
    match read_capped(body, limits.cap_bytes) {
        Ok(CappedRead::Complete(bytes)) => Ok(bytes),
        Ok(CappedRead::Exceeded) => Err(FetchFailure::ExceededCap),
        Err(e) => Err(FetchFailure::Read(e.to_string())),
    }
}

/// The streaming twin of [`get`]: copy the response body into `sink` under
/// `limits.cap_bytes`, refusing the source one byte past the cap — through
/// the wire kit's [`read_capped_into`], the bounded copy [`read_capped`]'s
/// own home provides. The buffered [`get`] is for a caller that must parse
/// the body in memory (`--fetch-params`); this one is for a caller that
/// stages the body to a file, so a bundle at the cap never lives in RAM
/// (and an out-of-memory abort cannot bypass the caller's cleanup).
pub(crate) fn get_into(
    url: &str,
    limits: &FetchLimits,
    sink: &mut impl Write,
) -> Result<(), FetchFailure> {
    let body = open(url, limits)?;
    match read_capped_into(body, sink, limits.cap_bytes) {
        CappedCopy::Complete(_) => Ok(()),
        CappedCopy::Exceeded => Err(FetchFailure::ExceededCap),
        CappedCopy::Source(e) => Err(FetchFailure::Read(e.to_string())),
        CappedCopy::Sink(e) => Err(FetchFailure::Write(e.to_string())),
    }
}

/// The one client every fetch here rides: the caller's global timeout and
/// its redirect-chain transport pin, built per call (as the release
/// channel's client is) so the pins are the only thing the request carries.
fn client(limits: &FetchLimits) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(limits.https_only)
            .timeout_global(Some(limits.timeout))
            .build(),
    )
}

// `get` is exercised end-to-end by the e2e suites' loopback fixtures
// (`tests/e2e_fetch_params.rs`, `tests/e2e_run_from_url.rs`).
#[cfg(test)]
mod tests {
    use super::*;

    /// The client's transport pin, asserted on the built config (the
    /// `release_channel::http` test's shape): a caller that sets
    /// `https_only` gets a client whose every redirect hop must be https.
    #[test]
    fn the_client_pins_https_only_when_the_caller_asks() {
        let limits = |https_only| FetchLimits {
            cap_bytes: 1024,
            timeout: Duration::from_secs(1),
            https_only,
        };
        assert!(client(&limits(true)).config().https_only());
        assert!(!client(&limits(false)).config().https_only());
    }

    /// The one shared wording over the five facts: only the cap sentence is
    /// the caller's.
    #[test]
    fn message_words_every_failure_arm() {
        let cap = "the thing exceeded its cap";
        assert_eq!(FetchFailure::Status(404).message(cap), "HTTP 404");
        assert_eq!(FetchFailure::ExceededCap.message(cap), cap);
        assert_eq!(
            FetchFailure::Transport("timed out".into()).message(cap),
            "request failed: timed out"
        );
        assert_eq!(
            FetchFailure::Read("reset".into()).message(cap),
            "reading response: reset"
        );
        assert_eq!(
            FetchFailure::Write("no space".into()).message(cap),
            "writing the response: no space"
        );
    }
}
