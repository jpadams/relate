//! The `dungeon` extension child (ADR-0431 §11): the game's MCP half.
//! Its whole input is one request envelope on stdin — the resolved flag
//! values the host parsed from the committed manifest's flag table —
//! and its whole output is the MCP session it drives as the client: the
//! `initialize` handshake, the turn's `read_cypher`/`write_cypher`
//! calls, and the turn's text through `report_output`.
//!
//! It carries no `Database` and never touches a store (ADR-0447 §4):
//! the host owns the store, the commit policy, the caps and the exit
//! codes; this child's only power is the calls it makes. The kit's
//! [`serve`](relate_wire::mcp::serve) owns the bootstrap — the
//! envelope read, the handshake, the failure report, the exit.

use relate_dungeon::run;
use relate_wire::mcp::{serve, StdioChild};
use serde::Deserialize;

/// The mount name the handshake reports — the committed manifest
/// declares the same package.
const NAME: &str = "dungeon";

/// The host's request envelope: one JSON line carrying the manifest's
/// resolved flag values. `command` is the turn (`--command`, default
/// `status`); the host resolves the default, so the field always
/// arrives.
#[derive(Debug, Deserialize)]
struct Envelope {
    command: String,
}

fn main() {
    serve(NAME, env!("CARGO_PKG_VERSION"), walk);
}

/// One command, one turn — the game's dispatch owns the session.
fn walk(child: &mut StdioChild, envelope: Envelope) -> Result<(), String> {
    run(child, &envelope.command)
}
