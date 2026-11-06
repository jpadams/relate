//! The 5.x session leg (ADR-0484): the handlers behind the 5.4 arms of
//! `Session`'s dispatch — the HELLO that no longer authenticates
//! (decision 3), LOGON and LOGOFF (decision 4) and TELEMETRY (decision 5).
//! A child of `session`, so the handlers keep touching the session's
//! private state; the gates that route messages here live in the parent.

use super::*;

impl Session {
    /// The 5.x HELLO (ADR-0484 decisions 3, 5 and 8): `bolt_agent` is
    /// required beside `user_agent`, the auth fields and `patch_bolt` are
    /// silently ignored (no patch negotiation exists at 5.x, so no
    /// acknowledgment key is sent), and the notifications filter keys
    /// configure the connection when they parse. A validated HELLO answers
    /// the SUCCESS metadata — the `telemetry.enabled: false` opt-out hint
    /// appended here, this module owning the 5.x decisions — and leaves
    /// the session awaiting LOGON; a session that was already open stays
    /// open (a re-HELLO is answered, never demoting it).
    pub(super) fn hello_5x<W: Write>(
        &mut self,
        stream: &mut W,
        fields: &[Pack],
        config: &ConnConfig,
    ) -> std::io::Result<Flow> {
        if !bolt_agent_is_string_map(fields) {
            return self.reject_hello(stream, BOLT_AGENT_REQUIRED);
        }
        let notifications = match NotificationConfig::parse(fields, 0) {
            Ok(config) => config,
            Err(parse_failure) => {
                return self.reject_hello(stream, &illegal_value_for_field(&parse_failure))
            }
        };
        self.notifications = notifications;
        if self.gate != Gate::Open {
            self.gate = Gate::AwaitingLogon;
        }
        // The telemetry opt-out hint is this leg's own contract (ADR-0484
        // decision 5): this handler runs only behind the speaks_5x fork,
        // so the hint rides every 5.x HELLO SUCCESS — `summary::hello`
        // stays version-free.
        let metadata = summary::hello(
            &config.server_identity,
            &self.connection_id,
            config.limits.keep_alive_interval,
            vec![summary::telemetry_opt_out_hint()],
        );
        send_success(stream, metadata)?;
        Ok(Flow::Continue)
    }

    /// LOGON (0x6A, ADR-0484 decision 4): the ADR-0126 credential check at
    /// its 5.1+ home. Reached only while the session awaits LOGON — the
    /// gates answer it everywhere else. `basic` authenticates against the
    /// resolved credential, and under `--auth none` every LOGON does (as
    /// every 4.4 HELLO does today, including scheme `none`); a mismatch or
    /// a scheme the config does not answer fails the probed Unauthorized
    /// FAILURE — after the ADR-0139 guessing delay — and closes the
    /// connection (DEFUNCT, per the 5.1 state table): the shared
    /// [`Session::authenticate`] sequence with this message's probed
    /// failure text. Success answers SUCCESS `{}` and opens the session,
    /// the verified principal reported engine-ward (ADR-0273).
    pub(super) fn logon<W: Write>(
        &mut self,
        stream: &mut W,
        fields: &[Pack],
        config: &ConnConfig,
    ) -> std::io::Result<Flow> {
        if let Some(failed) = self.authenticate(stream, fields, config, LOGON_UNAUTHORIZED)? {
            return Ok(failed);
        }
        send_success(stream, vec![])?;
        Ok(Flow::Continue)
    }

    /// LOGOFF (0x6B, ADR-0484 decision 4): from an open session, SUCCESS
    /// `{}` and back to awaiting LOGON. The gates answer it everywhere
    /// else — the AUTHENTICATION gate breaches it while the session awaits
    /// LOGON, the failed state IGNOREs it — so the handler only ever sees
    /// an open, unfailed session; engine state (an open transaction, a
    /// buffered result) is left as it stands.
    pub(super) fn logoff<W: Write>(&mut self, stream: &mut W) -> std::io::Result<Flow> {
        self.gate = Gate::AwaitingLogon;
        send_success(stream, vec![])?;
        Ok(Flow::Continue)
    }

    /// TELEMETRY (0x54, ADR-0484 decision 5): the server collects nothing,
    /// and the HELLO hint already said so — every in-range `api` Int is
    /// answered SUCCESS `{}` and nothing is recorded. An out-of-range Int
    /// answers the probed UnknownError (Neo4j's packstream-reader text
    /// naming the value), a non-Int `api` the probed Request.Invalid; both
    /// leave the session in the failed state, the manual's TELEMETRY
    /// failure transition, so RESET recovers.
    pub(super) fn telemetry<W: Write>(
        &mut self,
        stream: &mut W,
        fields: &[Pack],
    ) -> std::io::Result<Flow> {
        match fields.first() {
            Some(Pack::Int(api)) if (0..=3).contains(api) => {
                send_success(stream, vec![])?;
            }
            Some(Pack::Int(api)) => {
                return self.fail_with_code(
                    stream,
                    UNKNOWN_ERROR_CODE,
                    &format!(
                        "org.neo4j.packstream.error.reader.PackstreamReaderException: \
                         Unknown driver interface type {api}"
                    ),
                );
            }
            Some(other) => {
                return self.fail_with_code(
                    stream,
                    REQUEST_INVALID_CODE,
                    // Neo4j names the packstream kinds in upper case (the
                    // probed "… but got STRING").
                    &format!(
                        "Unexpected type: Expected INT but got {}",
                        other.kind_name().to_uppercase()
                    ),
                );
            }
            None => {
                return self.fail_with_code(
                    stream,
                    REQUEST_INVALID_CODE,
                    "missing TELEMETRY api (TELEMETRY api must be an Int)",
                );
            }
        }
        Ok(Flow::Continue)
    }
}
