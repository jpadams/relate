//! The live telemetry handle (ADR-0079), compiled with the `telemetry`
//! feature.
//!
//! Shape: [`Telemetry::new`] resolves the opt-out layers, loads (or creates)
//! the install ID, and — when enabled — spawns a single background thread
//! that drains a bounded channel (capacity 64, drop-on-overflow) and POSTs
//! events to Mixpanel as JSON arrays. [`Telemetry::emit`] is a `try_send`:
//! it never blocks. [`Telemetry::flush`] closes the channel and waits —
//! bounded by a 250 ms budget, not the worker's HTTP timeout — for the
//! worker to batch whatever is pending into one final POST; a request
//! still in flight past the budget is dropped and the worker left
//! detached, so no exit is ever perceptibly slowed (ADR-0079 §5). Every
//! failure — config I/O, HTTP,
//! serialization — is swallowed: no panics and no output, with one
//! sanctioned exception: the ADR-0263 rejection warning described below is
//! a single stderr line.
//!
//! Endpoint resolution: when `RELATE_MIXPANEL_ENDPOINT` is set it is
//! used as the full POST URL, but ADR-0263 rejects a non-loopback `http://`
//! value — a cleartext endpoint would leak events — and a rejected override
//! disables telemetry for the run with one stderr warning. Otherwise the
//! compiled-in [`DEFAULT_ENDPOINT`] is used. Likewise
//! `RELATE_MIXPANEL_TOKEN` overrides the compiled-in
//! [`MIXPANEL_TOKEN`] (ADR-0079 §7).

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use serde_json::Value;

use super::envelope::{self, BaseContext};
use super::{config, optout, EffectiveState, Event};
use crate::plaintext::{authority_host, is_loopback_name};

/// Default Mixpanel `/track` endpoint (ADR-0079 §2). Used when
/// `RELATE_MIXPANEL_ENDPOINT` is unset; an override is accepted only if
/// it is `https://` for any host or `http://` for a loopback host (ADR-0263).
pub const DEFAULT_ENDPOINT: &str = "https://api.mixpanel.com/track";

/// Mixpanel project token (ADR-0079 §7): the shared Neo4j Labs CLI project —
/// the same one neo4j-cli reports into; events are namespaced by the
/// `RELATE-CLI_` prefix so the tools coexist. This is a client-side
/// ingestion token, public in every shipped binary by design. Overridable
/// via `RELATE_MIXPANEL_TOKEN`.
pub const MIXPANEL_TOKEN: &str = "4bfb2414ab973c741b6f067bf06d5575";

/// Bounded channel capacity; `emit` drops events beyond this (ADR-0079 §5).
const CHANNEL_CAPACITY: usize = 64;

/// Per-request HTTP timeout for the background worker (ADR-0079 §5).
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the exit flush waits for the worker's final batch before
/// detaching — the bound behind ADR-0079 §5's "never perceptibly slowed".
/// A final request still in flight past the budget is dropped, exactly
/// like an overflowed channel: delivery is best-effort, the exit is never
/// slow. `RELATE_TELEMETRY_FLUSH_MS` can lower it (to 0,
/// fire-and-forget) for loop-shaped callers and test harnesses; it is
/// bound once at construction, through the same env seam as every other
/// knob.
const FLUSH_BUDGET: Duration = Duration::from_millis(250);

/// The exit-flush wait budget from the environment:
/// `RELATE_TELEMETRY_FLUSH_MS` milliseconds when set (0 =
/// fire-and-forget), with surrounding whitespace ignored; an unset or
/// unparsable value is the default — telemetry never prints (ADR-0079 §5).
fn flush_budget(env: &impl Fn(&str) -> Option<String>) -> Duration {
    env("RELATE_TELEMETRY_FLUSH_MS")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(FLUSH_BUDGET)
}

/// The enabled pipeline: the event base context, the bounded channel, the
/// worker's completion signal, and the flush budget. One value, so the
/// four can never drift apart. The worker thread itself is detached at
/// spawn: the [`Receiver`] is how [`Telemetry::flush`](Telemetry) waits
/// for it, bounded by the budget — joining it directly would hand the
/// worker's HTTP timeout control of every exit.
struct Live {
    base: BaseContext,
    sender: SyncSender<Value>,
    done: Receiver<()>,
    budget: Duration,
}

/// Telemetry handle. Cheap to keep around when disabled: no thread, no
/// state file, no network. Dropping without [`flush`](Self::flush) flushes.
pub struct Telemetry {
    state: EffectiveState,
    /// The live pipeline, present exactly when telemetry is enabled: base
    /// context, channel, and worker are born together in `spawn` and torn
    /// down together in `flush` — one `Option`, so a half-built handle is
    /// not representable.
    live: Option<Live>,
    /// First-run-notice policy state (ADR-0079 §6): the config dir and the
    /// `state.json` contents loaded once at construction. `None` when
    /// telemetry is disabled or no config dir resolved, in which case
    /// [`claim_first_run_notice`](Self::claim_first_run_notice) never claims
    /// and nothing is written.
    notice: Option<(std::path::PathBuf, config::State)>,
}

impl Telemetry {
    /// Resolve the opt-out layers (`DO_NOT_TRACK` → `RELATE_TELEMETRY` →
    /// `config.json`), load or create the install ID under the XDG config
    /// directory, and — when enabled — spawn the background worker. Never
    /// fails; the only output is the single ADR-0263 stderr warning when a
    /// cleartext non-loopback endpoint override is rejected.
    pub fn new() -> Self {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let dir = crate::config_dir::config_dir();
        let config_telemetry = dir.as_deref().and_then(config::load_config_telemetry);
        let state = optout::resolve(&env, config_telemetry);
        // When disabled, touch nothing: no state file, no thread, no network.
        if !state.enabled {
            return Self::disabled(state);
        }
        Self::enable(state, &env, dir)
    }

    /// The enabled path of [`new`](Self::new), split out so tests can drive
    /// endpoint-override rejection with an explicit env and config dir.
    /// Endpoint resolution runs before `load_notice_state` so a rejected
    /// override leaves no `state.json` behind — exactly like the opt-out
    /// early return in [`new`](Self::new).
    fn enable(
        state: EffectiveState,
        env: &impl Fn(&str) -> Option<String>,
        dir: Option<std::path::PathBuf>,
    ) -> Self {
        // A rejected cleartext non-loopback override disables telemetry for
        // the run rather than silently redirecting or leaking events
        // (ADR-0263); the opt-out state is reported unchanged.
        let endpoint = match resolve_endpoint(env("RELATE_MIXPANEL_ENDPOINT")) {
            Ok(url) => url,
            Err(url) => {
                eprintln!("{}", rejection_warning(&url));
                return Self::disabled(state);
            }
        };
        // The state file is loaded exactly once, here: the install ID feeds
        // the base context and `notice_shown` feeds claim_first_run_notice.
        let (install_id, notice) = load_notice_state(dir);
        let token = env("RELATE_MIXPANEL_TOKEN").unwrap_or_else(|| MIXPANEL_TOKEN.to_string());
        Self::spawn(
            state,
            BaseContext::detect(token, install_id),
            endpoint,
            notice,
            flush_budget(env),
        )
    }

    fn disabled(state: EffectiveState) -> Self {
        Self {
            state,
            live: None,
            notice: None,
        }
    }

    fn spawn(
        state: EffectiveState,
        base: BaseContext,
        endpoint: String,
        notice: Option<(std::path::PathBuf, config::State)>,
        budget: Duration,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Value>(CHANNEL_CAPACITY);
        let (done_tx, done) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            worker_loop(rx, endpoint);
            // The only completion signal: sent after the final batch, so
            // flush's bounded wait ends as soon as the worker is really
            // done. A send failure means flush already timed out and
            // dropped its end — the worker was left to finish detached.
            let _ = done_tx.send(());
        });
        Self {
            state,
            live: Some(Live {
                base,
                sender: tx,
                done,
                budget,
            }),
            notice,
        }
    }

    /// The resolved state and which layer decided it (for
    /// `relate telemetry status`, ADR-0079 §6).
    pub fn effective_state(&self) -> EffectiveState {
        self.state
    }

    /// One-time first-run-notice policy (ADR-0079 §6): returns `true`
    /// exactly once per install — on the first run with telemetry enabled,
    /// interactive or not — setting `notice_shown` in `state.json` as it
    /// does. Returns `false` (and writes nothing) when telemetry is
    /// disabled by any opt-out layer, when the notice was already shown, or
    /// when no config dir resolved.
    pub fn claim_first_run_notice(&mut self) -> bool {
        if !self.state.enabled {
            return false;
        }
        let Some((dir, state)) = &mut self.notice else {
            return false;
        };
        if state.notice_shown {
            return false;
        }
        state.notice_shown = true;
        let _ = config::save_state(dir, state);
        true
    }

    /// Queue an event for delivery. Non-blocking: a full channel drops the
    /// event; after [`flush`](Self::flush) (or when disabled) this is a safe
    /// no-op. Never panics, never prints.
    pub fn emit(&self, event: Event) {
        let Some(live) = &self.live else {
            return;
        };
        let envelope = envelope::build_envelope(&event, &live.base);
        // Full or disconnected channel: drop the event (ADR-0079 §5).
        let _ = live.sender.try_send(envelope);
    }

    /// Drain and send whatever is pending — batched into at most one final
    /// POST — then return. Bounded by the handle's flush budget (250 ms
    /// default, `RELATE_TELEMETRY_FLUSH_MS` at construction, 0 =
    /// fire-and-forget): a final request still in flight past the budget
    /// is dropped and the worker left detached, per ADR-0079 §5's
    /// best-effort delivery. Idempotent: a second call returns immediately,
    /// and `emit` afterwards is a no-op.
    pub fn flush(&mut self) {
        // Closing the channel makes the worker drain, send one final batch,
        // and signal completion; the bounded wait ends on that signal or
        // the budget, whichever comes first.
        if let Some(Live {
            sender,
            done,
            budget,
            ..
        }) = self.live.take()
        {
            drop(sender);
            let _ = done.recv_timeout(budget);
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Load `state.json` under `dir` exactly once, returning the install ID and
/// the notice-policy state kept on the handle. With no resolvable config
/// dir the install ID is an ephemeral per-process UUID and the notice can
/// never be claimed.
fn load_notice_state(
    dir: Option<std::path::PathBuf>,
) -> (String, Option<(std::path::PathBuf, config::State)>) {
    match dir {
        Some(d) => {
            let file_state = config::load_or_create_state(&d);
            (file_state.install_id.clone(), Some((d, file_state)))
        }
        None => (envelope::uuid_v4(), None),
    }
}

/// Drain the channel, POSTing batches until the sender hangs up. `recv`
/// still returns already-queued events after the sender is dropped, so a
/// flush drains everything pending before the loop exits.
fn worker_loop(rx: Receiver<Value>, endpoint: String) {
    let agent = http_agent();
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        post_batch(&agent, &endpoint, &batch);
    }
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .build(),
    )
}

/// POST one batch of envelopes as a JSON array (the Mixpanel `/track` API
/// accepts arrays). All errors are swallowed (ADR-0079 §5).
fn post_batch(agent: &ureq::Agent, endpoint: &str, batch: &[Value]) {
    let Ok(body) = serde_json::to_vec(batch) else {
        return;
    };
    let _ = agent
        .post(endpoint)
        .header("Content-Type", "application/json")
        .send(body.as_slice());
}

/// Resolve the endpoint to POST to (ADR-0263): the override when it passes
/// [`endpoint_allowed`], the default when unset, or `Err(url)` for a rejected
/// override that the caller must refuse (disable telemetry + warn).
fn resolve_endpoint(raw: Option<String>) -> Result<String, String> {
    match raw {
        None => Ok(DEFAULT_ENDPOINT.to_string()),
        Some(url) if endpoint_allowed(&url) => Ok(url),
        Some(url) => Err(url),
    }
}

/// Whether an override is a safe endpoint (ADR-0263): `https://` for any
/// host, `http://` only for a loopback host. Scheme and host compare
/// case-insensitively and an IPv6 literal's brackets are trimmed, so
/// `HTTPS://sink.example` and `http://[::1]:8080` are allowed while a
/// schemeless, unrecognized-scheme, or non-loopback cleartext value is not.
fn endpoint_allowed(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if scheme.eq_ignore_ascii_case("https") {
        return true;
    }
    if !scheme.eq_ignore_ascii_case("http") {
        return false;
    }
    let host = authority_host(rest);
    is_loopback_name(host) || host == "127.0.0.1"
}

/// The single stderr warning emitted when an override is rejected (ADR-0263
/// pins this text). `<url>` is the rejected value, reproduced verbatim.
fn rejection_warning(url: &str) -> String {
    format!(
        "warning: ignoring RELATE_MIXPANEL_ENDPOINT={url}: http:// is allowed only for loopback hosts; telemetry disabled for this run"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{Layer, StartupKind};
    use std::time::Instant;

    fn enabled() -> EffectiveState {
        EffectiveState {
            enabled: true,
            layer: Layer::Default,
        }
    }

    fn disabled_state() -> EffectiveState {
        EffectiveState {
            enabled: false,
            layer: Layer::Env,
        }
    }

    /// An enabled handle against `endpoint`, built through `enable`'s env
    /// seam — the one resolution path production runs — so tests exercise
    /// the same wiring `new` uses instead of restating spawn's parts.
    fn at_endpoint(endpoint: &str) -> Telemetry {
        let pairs = [("RELATE_MIXPANEL_ENDPOINT", endpoint)];
        let env = crate::telemetry::env_of(&pairs);
        Telemetry::enable(enabled(), &env, None)
    }

    fn refused() -> Telemetry {
        // Port 1 refuses connections immediately.
        at_endpoint("http://127.0.0.1:1")
    }

    #[test]
    fn flush_budget_trims_and_falls_back_to_default() {
        // The knob is normalized before parsing — " 0" is the fire-and-forget
        // escape hatch, not a silent fallback to the 250 ms default — and
        // anything still unparsable after trimming is the default, silently:
        // telemetry never prints (ADR-0079 §5).
        let cases: &[(&str, Duration)] = &[
            ("0", Duration::ZERO),
            (" 0 ", Duration::ZERO),
            ("10000", Duration::from_secs(10)),
            ("not-a-number", FLUSH_BUDGET),
        ];
        for (raw, want) in cases {
            let pairs = [("RELATE_TELEMETRY_FLUSH_MS", *raw)];
            let env = crate::telemetry::env_of(&pairs);
            assert_eq!(flush_budget(&env), *want, "raw value {raw:?}");
        }
        let none = crate::telemetry::env_of(&[]);
        assert_eq!(flush_budget(&none), FLUSH_BUDGET, "unset knob");
    }

    /// A sink that accepts connections but never responds, as an endpoint
    /// URL. Connect is instant on every platform (unlike a refused port,
    /// whose failure timing is OS-dependent — ~2s of SYN retransmission on
    /// Windows), so a POST to it hangs for the worker's full HTTP timeout.
    /// The accept loop runs in a detached thread that keeps every accepted
    /// socket open without reading or writing; the listener is moved into
    /// that thread, which blocks in accept() until the test process exits —
    /// the thread and its handful of FDs leak until then, which is harmless
    /// for a test.
    fn hanging_sink() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept() {
                held.push(stream);
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn flush_is_bounded_against_hanging_endpoint() {
        let mut t = at_endpoint(&hanging_sink());
        for _ in 0..10 {
            t.emit(Event::Help);
        }
        let start = Instant::now();
        t.flush();
        // Bound: the final POST hangs, so the worker never signals
        // completion and flush returns at the budget, not the worker's
        // 2x-HTTP_TIMEOUT worst case. The 1s margin covers scheduling
        // overhead on loaded CI runners.
        assert!(
            start.elapsed() < FLUSH_BUDGET + Duration::from_secs(1),
            "flush took {:?} against a hanging endpoint",
            start.elapsed()
        );
    }

    #[test]
    fn flush_returns_promptly_when_the_worker_finishes_in_budget() {
        // The happy path of the done channel: when the worker finishes,
        // flush returns on the completion signal, not by out-waiting the
        // budget. With no events emitted there is no POST, so the worker
        // runs to completion and signals as soon as the sender drops —
        // deterministically, with no OS-dependent connection-failure
        // timing in the path. The budget is set far above the assert
        // bound, so a broken signal (flush falling back to the budget
        // timeout) cannot pass: it would hold flush for the full 10 s.
        let pairs = [
            ("RELATE_MIXPANEL_ENDPOINT", "http://127.0.0.1:1"),
            ("RELATE_TELEMETRY_FLUSH_MS", "10000"),
        ];
        let env = crate::telemetry::env_of(&pairs);
        let mut t = Telemetry::enable(enabled(), &env, None);
        let start = Instant::now();
        t.flush();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "flush took {:?} with a 10s budget and an idle worker",
            start.elapsed()
        );
    }

    #[test]
    fn a_zero_flush_budget_is_fire_and_forget() {
        // The escape hatch for loop-shaped callers and test harnesses:
        // RELATE_TELEMETRY_FLUSH_MS=0 makes flush return immediately
        // without waiting for the worker at all. Driven through enable's
        // env seam with an injected env — the same resolution every other
        // knob test uses — so no process-global env is touched.
        let endpoint = hanging_sink();
        let pairs = [
            ("RELATE_MIXPANEL_ENDPOINT", endpoint.as_str()),
            ("RELATE_TELEMETRY_FLUSH_MS", "0"),
        ];
        let env = crate::telemetry::env_of(&pairs);
        let mut t = Telemetry::enable(enabled(), &env, None);
        t.emit(Event::Help);
        let start = Instant::now();
        t.flush();
        // The worker hangs in its final POST for the full HTTP timeout
        // while flush has already returned. 1 s margin, not the bare
        // budget, for the same CI-scheduling reasons as the sibling
        // timing tests.
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "flush took {:?} with a zero budget",
            start.elapsed()
        );
    }

    #[test]
    fn emit_is_nonblocking_and_drops_on_overflow() {
        // A sink that accepts connections but never responds: the worker is
        // stuck inside a 2s-timeout POST while we emit far beyond capacity.
        let mut t = at_endpoint(&hanging_sink());
        let start = Instant::now();
        for _ in 0..(CHANNEL_CAPACITY * 4) {
            t.emit(Event::Help);
        }
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "emit blocked for {:?} with a stuck worker",
            start.elapsed()
        );
        // Flush returns within the handle's flush budget (250 ms here)
        // regardless of the worker's stuck in-flight request; the worker
        // finishes detached, bounded by its own HTTP timeout.
        t.flush();
    }

    #[test]
    fn disabled_telemetry_swallows_everything() {
        let mut t = Telemetry::disabled(disabled_state());
        assert!(!t.effective_state().enabled);
        assert_eq!(t.effective_state().layer, Layer::Env);
        t.emit(Event::Help); // no-op
        t.flush(); // returns immediately: no worker
    }

    #[test]
    fn emit_after_flush_is_a_safe_noop() {
        let mut t = refused();
        t.emit(Event::Startup {
            kind: StartupKind::Oneshot,
        });
        t.flush();
        // No panic, no output, no blocking.
        t.emit(Event::Help);
        let start = Instant::now();
        t.flush();
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn effective_state_is_reported() {
        let t = refused();
        assert_eq!(t.effective_state(), enabled());
    }

    #[test]
    fn endpoint_allowed_accepts_https_for_any_host() {
        assert!(endpoint_allowed("https://api.mixpanel.com/track"));
        assert!(endpoint_allowed("https://sink.example:8443/track?x=1"));
        assert!(endpoint_allowed("HTTPS://api.mixpanel.com/track"));
    }

    #[test]
    fn endpoint_allowed_accepts_loopback_http_only() {
        assert!(endpoint_allowed("http://127.0.0.1:8080"));
        assert!(endpoint_allowed("http://localhost:8080/track"));
        assert!(endpoint_allowed("http://[::1]:8080"));
        assert!(endpoint_allowed("HTTP://LOCALHOST/track"));
        assert!(!endpoint_allowed("http://api.mixpanel.com/track"));
        assert!(!endpoint_allowed("http://10.0.0.1:8080"));
        assert!(!endpoint_allowed("HTTP://example.com"));
    }

    #[test]
    fn endpoint_allowed_rejects_schemeless_and_garbage() {
        assert!(!endpoint_allowed("api.mixpanel.com/track"));
        assert!(!endpoint_allowed("not a url"));
        assert!(!endpoint_allowed(""));
        assert!(!endpoint_allowed("ftp://127.0.0.1/track"));
    }

    #[test]
    fn resolve_endpoint_defaults_and_rejects() {
        assert_eq!(resolve_endpoint(None).unwrap(), DEFAULT_ENDPOINT);
        let allowed = "https://sink.example/track".to_string();
        assert_eq!(resolve_endpoint(Some(allowed.clone())).unwrap(), allowed);
        let rejected = "http://evil.example/track".to_string();
        assert_eq!(
            resolve_endpoint(Some(rejected.clone())).unwrap_err(),
            rejected
        );
    }

    #[test]
    fn rejection_warning_pins_the_adr_text() {
        assert_eq!(
            rejection_warning("http://evil.example/track"),
            "warning: ignoring RELATE_MIXPANEL_ENDPOINT=http://evil.example/track: http:// is allowed only for loopback hosts; telemetry disabled for this run"
        );
    }

    #[test]
    fn rejected_endpoint_writes_no_state_file() {
        // Rejection must behave like an opt-out: the endpoint is resolved
        // before any state-file I/O, so no `state.json` is created.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let env = |k: &str| {
            (k == "RELATE_MIXPANEL_ENDPOINT").then(|| "http://evil.example/track".to_string())
        };
        let t = Telemetry::enable(enabled(), &env, Some(dir.to_path_buf()));
        // The opt-out state is reported unchanged, but nothing runs.
        assert!(t.live.is_none());
        assert!(!config::state_path(dir).exists());
    }

    #[test]
    fn rejected_override_disables_without_a_worker() {
        // The rejection path reuses `Self::disabled`: no sender/worker, and
        // the opt-out state is reported unchanged. `emit`/`flush` no-op.
        let mut t = Telemetry::disabled(enabled());
        assert_eq!(t.effective_state(), enabled());
        assert!(t.live.is_none());
        t.emit(Event::Help);
        t.flush();
    }

    /// An enabled handle whose notice policy resolves against `dir`, built
    /// through `enable`'s env seam so the state-file wiring is the one
    /// production path.
    fn enabled_in_dir(dir: &std::path::Path) -> Telemetry {
        let pairs = [("RELATE_MIXPANEL_ENDPOINT", "http://127.0.0.1:1")];
        let env = crate::telemetry::env_of(&pairs);
        Telemetry::enable(enabled(), &env, Some(dir.to_path_buf()))
    }

    #[test]
    fn first_run_notice_shown_exactly_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let mut t = enabled_in_dir(dir);
        assert!(t.claim_first_run_notice(), "first check shows the notice");
        assert!(
            !t.claim_first_run_notice(),
            "second check is suppressed by the notice_shown flag"
        );
        assert!(config::load_or_create_state(dir).notice_shown);
        // A later process (fresh handle, same dir) sees the persisted flag.
        let mut t2 = enabled_in_dir(dir);
        assert!(!t2.claim_first_run_notice());
    }

    #[test]
    fn first_run_notice_suppressed_when_disabled() {
        // The disabled layer must suppress the claim even when notice state
        // exists and is claimable: the handle carries the state a loaded
        // `dir` would have produced, so the false returns below prove the
        // enabled check fires — not that no notice was ever present — and
        // the absent state file proves the suppression happens before the
        // notice-saving write.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let mut t = Telemetry::disabled(disabled_state());
        t.notice = Some((
            dir.to_path_buf(),
            config::State {
                install_id: "test-install-id".to_string(),
                notice_shown: false,
            },
        ));
        assert!(!t.claim_first_run_notice());
        assert!(!t.claim_first_run_notice());
        assert!(!config::state_path(dir).exists());
    }
}
