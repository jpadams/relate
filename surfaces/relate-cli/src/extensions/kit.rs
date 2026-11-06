//! The canonical bash MCP client kit (ADR-0447 §4): the one text a bash
//! child carries.
//!
//! `docs/api/extensions/examples/mcp-client.sh` is the sourceable form — a
//! shebang and a sourcing note, then the kit. A package ships exactly one
//! executable, so a bash package inlines the body: the shipped example
//! (`so-to-graph.sh`) does, `scripts/check-extension-examples.sh` pins the
//! two copies together, and `relate extension init --lang sh` generates
//! it. The body starts at the kit's own first line ([`MCP_KIT_START`]), past
//! the sourceable file's header.
//!
//! One reader of the one file: the scaffold ([`crate::extension_init`]) and
//! the fixture kit ([`super::testkit`]) both inline this text, so a kit edit
//! reaches the shipped example, the generated packages and the fixtures
//! together.

/// The canonical kit file, read at build time — never copied into the
/// binary's sources, so there is no second copy to drift.
const MCP_CLIENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/api/extensions/examples/mcp-client.sh"
));

/// The line the kit's body starts at — past `mcp-client.sh`'s header. The
/// same boundary `scripts/check-extension-examples.sh` pins the example's
/// inline against.
const MCP_KIT_START: &str = "# The MCP revision this kit speaks";

/// The bash MCP kit a package inlines: the canonical file past its header,
/// i.e. the exact text a package carries.
pub(crate) fn mcp_kit() -> &'static str {
    let at = MCP_CLIENT
        .find(MCP_KIT_START)
        .expect("mcp-client.sh carries the kit below its header");
    &MCP_CLIENT[at..]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kit's body is the whole file past the header: the boundary is a
    /// fact of the canonical file, not a slice someone tuned here.
    #[test]
    fn the_kit_starts_at_its_own_first_line() {
        let kit = mcp_kit();
        assert!(kit.starts_with(MCP_KIT_START), "{kit:.80}");
        assert!(kit.contains("mcp_init() {"), "the handshake is in the kit");
        assert!(kit.contains("mcp_call() {"), "the tool call is in the kit");
        assert!(
            !kit.contains("#!/usr/bin/env bash"),
            "the shebang is the sourceable header, not the kit a package carries"
        );
    }
}
