//! The stdio transport (ADR-0232 §4): newline-delimited JSON-RPC 2.0 over
//! the process's stdin/stdout. MCP hosts (Claude Desktop, Claude Code)
//! spawn the server as a child process and speak this framing. There is no
//! auth challenge here — the parent spawned us, so there is no trust
//! boundary to cross (ADR-0232 §1).
//!
//! Generic over `Read`/`Write` so tests and the CLI can drive it over
//! in-memory pipes as well as the real stdio.

use std::io::{BufRead, Write};

use crate::server::{McpServer, ServeError};

/// Serve newline-delimited JSON-RPC on `reader`/`writer` until EOF.
/// Blank lines are ignored; every request line gets exactly one response
/// line; notifications get none.
pub fn serve_stdio(
    server: &McpServer,
    reader: impl BufRead,
    mut writer: impl Write,
) -> Result<(), ServeError> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_message(&line) {
            writer.write_all(response.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServerConfig;
    use relate::Database;

    #[test]
    fn serves_requests_and_skips_notifications() {
        let db = Database::open(":memory:").unwrap();
        let server = McpServer::new(db, McpServerConfig::default());
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n"
        );
        let mut output = Vec::new();
        serve_stdio(&server, input.as_bytes(), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].contains("\"serverInfo\""));
        assert!(lines[1].contains("\"id\":2"));
    }
}
