//! The engine→surface error mapping for the MCP tools (ADR-0260's trust
//! boundary, mirroring the HTTP Query API). A leaf module both `tools` and
//! `schema` use, so the crate graph stays acyclic (server → tools →
//! schema; errors is shared leafward).

/// An engine error as tool-error text: the message the engine's
/// classification allows across (ADR-0260, single-sourced in
/// `relate_error::client_message`) — internal-class detail (filesystem
/// paths, storage internals, panic payloads, and the Storage/Io/Locked
/// family, not just `Error::Internal`) is genericized before it can reach
/// an MCP host's tool-call logs; client-facing errors pass through
/// verbatim.
pub(crate) fn engine_error_text(err: &relate::Error) -> String {
    // The bare client-safe message; the ToolError renderer owns the
    // `Error: ` framing (one renderer, framed exactly once).
    relate::client_message(err).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_engine_errors_are_genericized_in_tool_errors() {
        // ADR-0260's trust boundary: internal detail (paths, storage
        // internals, panic payloads) never reaches the host's tool logs —
        // for the whole internal class, not just `Error::Internal`.
        for err in [
            relate::Error::Internal("worker panic at /secret/store/wal".into()),
            relate::Error::storage("checkpoint /secret/store/wal failed"),
            relate::Error::Locked("/secret/store/wal".into()),
        ] {
            let text = engine_error_text(&err);
            assert_eq!(text, relate::INTERNAL_MESSAGE);
            assert!(!text.contains("secret"), "{text}");
        }
        let client = engine_error_text(&relate::Error::syntax("bad query"));
        assert!(client.contains("bad query"), "{client}");
    }
}
