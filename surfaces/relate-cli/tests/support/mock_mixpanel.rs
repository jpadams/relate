//! Mock Mixpanel `/track` sink for the telemetry E2E suites: the canonical
//! loopback capture server (`relate-test-support`'s
//! [`spawn_capture_server`]) answers every POST on 127.0.0.1 (ephemeral
//! port) with `200 OK` and body `1`, exactly like the real API, and records
//! each request body. This module adds only the Mixpanel-specific residue:
//! envelope flattening, event polling, and the quiet-grace assertion.
//!
//! Every test binary that includes `support` compiles this module but uses a
//! subset of it, so dead-code warnings here would only be noise.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use relate_test_support::loopback::{spawn_capture_server, CaptureSink};

/// How long to wait for flushed events to arrive at the mock sink. The
/// binary's exit flush is bounded (ADR-0079 §5): it waits for the worker
/// only up to the flush budget, and `std::process::exit` then terminates
/// the worker thread — so events observed after the child exits are only
/// requests already written to the socket; this tolerates that delivery
/// timing.
pub const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Grace period for the opt-out tests: a disabled run makes no connections
/// at all, so anything arriving within this window is a failure.
pub const QUIET_GRACE: Duration = Duration::from_millis(500);

/// A minimal mock of the Mixpanel `/track` endpoint (see the module docs).
/// Dropping the mock stops the server and joins its thread.
pub struct MockMixpanel {
    sink: CaptureSink,
    port: u16,
}

impl MockMixpanel {
    pub fn start() -> Self {
        // The real `/track` API replies `200 OK` with body `1`.
        let (sink, port) = spawn_capture_server("200 OK", "1");
        Self { sink, port }
    }

    /// The URL to hand to `RELATE_MIXPANEL_ENDPOINT`.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn request_count(&self) -> usize {
        self.sink.request_count()
    }

    /// All captured events: every request body is a JSON array of Mixpanel
    /// envelopes; flatten them into one event list.
    pub fn events(&self) -> Vec<serde_json::Value> {
        self.sink
            .bodies()
            .iter()
            .flat_map(|body| {
                let value: serde_json::Value = serde_json::from_str(body)
                    .unwrap_or_else(|e| panic!("mock received a non-JSON body: {e}: {body}"));
                value
                    .as_array()
                    .expect("/track body must be a JSON array")
                    .clone()
            })
            .collect()
    }

    /// Poll the captured events until `pred` holds or `timeout` passes.
    pub fn wait_for_events(
        &self,
        timeout: Duration,
        pred: impl Fn(&[serde_json::Value]) -> bool,
    ) -> bool {
        let start = Instant::now();
        loop {
            if pred(&self.events()) {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Assert that no request arrives within [`QUIET_GRACE`]. `what` names
    /// the opted-out context being asserted (e.g. `DO_NOT_TRACK=1`) so a
    /// failure says which layer leaked.
    pub fn assert_quiet(&self, what: &str) {
        std::thread::sleep(QUIET_GRACE);
        assert_eq!(
            self.request_count(),
            0,
            "{what} must send no requests; mock captured: {:?}",
            self.sink.bodies()
        );
    }
}
