//! The extension fixture kit (LABS-470's test pattern, shared by the
//! host and run-mount suites, the registry's own discovery tests, and
//! the mount's own suites): a package installed under a tempdir
//! `$XDG_CONFIG_HOME`, its manifest text, and its shell child — loaded
//! through the injectable registry (`load_with`), so no test reads the
//! real extensions dir.
//!
//! The fixture child speaks the bash MCP kit ([`mcp_kit`], the canonical
//! `docs/api/extensions/examples/mcp-client.sh`, the same text the shipped
//! example carries): [`mcp_child_body`] composes a child's script from the
//! kit, its handshake and the test's own calls, so a fixture cannot forget
//! the handshake or drift from the kit.
//!
//! The kit's path joins read the walk's own facts —
//! [`crate::config_dir::CONFIG_DIR_NAME`], the registry's
//! `EXTENSIONS_DIR` and `MANIFEST_FILE` — so a fixture install and the
//! registry walk it feeds cannot disagree about where a package lives
//! or what it carries.

use crate::config_dir::CONFIG_DIR_NAME;
use crate::extensions::kit::mcp_kit;
use crate::extensions::registry::{ActionRegistry, ExtensionEntry, EXTENSIONS_DIR, MANIFEST_FILE};

/// The shipped bash example the kit's drift guard reads: the canonical copy
/// a package author starts from (a package is exactly two files, so the kit
/// rides inline).
const SHIPPED_EXAMPLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/api/extensions/examples/so-to-graph.sh"
));

/// A command manifest for `name` — the fixture every test installs:
/// `flags` spliced into the flag-table slot, `extra` appended before the
/// closing brace (a positional, `requires-fresh-store`, a commit
/// policy, `hidden`).
pub(crate) fn manifest_for(name: &str, flags: &str, extra: &str) -> String {
    format!(
        r#"{{"kind":"command","name":"{name}","version":"0.1.0","protocol":"2025-06-18","flags":[{flags}],"commit":"per-batch","executable":"run.sh"{extra}}}"#
    )
}

/// The every-kind flag table the parse tests resolve against.
pub(crate) const EVERY_KIND_FLAGS: &str = r#"
    {"kind": "string", "name": "message", "default": "hello"},
    {"kind": "integer", "name": "count", "default": 3},
    {"kind": "boolean", "name": "shout", "default": false},
    {"kind": "repeatable", "name": "tag", "default": ["a", "b"]}"#;

/// A fixture child's whole script: the bash MCP kit, the `initialize`
/// handshake under `name`, then `body` — the test's own `tools/call`s. One
/// composition, so every fixture opens the session the same way.
pub(crate) fn mcp_child_body(name: &str, body: &str) -> String {
    format!("{}\nmcp_init {name} 0.1.0\n{body}", mcp_kit())
}

/// Write one fixture child into `dir` and make it executable where the
/// platform has the exec bit. The shebang is bash: the kit's JSON scratches
/// use `${…//…}` and `local`, which `sh` implementations outside bash do
/// not all carry (ADR-0447's language proof is the shell itself, not any
/// one shell).
pub(crate) fn sh_fixture(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/usr/bin/env bash\n{body}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut mode = std::fs::metadata(&path).unwrap().permissions();
        mode.set_mode(0o755);
        std::fs::set_permissions(&path, mode).unwrap();
    }
    path
}

/// Install a package directory (and its `extension.json`) under `tmp`'s
/// extensions dir — joined through the walk's own consts, so the kit's
/// fixture trees and the registry walk they feed share one spelling of
/// where a package lives — and return the package directory for the
/// caller to attach the child it declares: `$XDG_CONFIG_HOME` is the
/// temp dir, so the walk reads `<tmp>/relate/extensions/<dir_name>/`
/// (ADR-0429 §2).
pub(crate) fn install_package(
    tmp: &tempfile::TempDir,
    dir_name: &str,
    manifest: &str,
) -> std::path::PathBuf {
    let dir = tmp
        .path()
        .join(CONFIG_DIR_NAME)
        .join(EXTENSIONS_DIR)
        .join(dir_name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    dir
}

/// Install a package with `manifest_text` and an executable `run.sh`
/// child with `body` under a tempdir `$XDG_CONFIG_HOME`, and load the
/// registry through the pinned env — the injectable-registry fixture
/// pattern (ADR-0429 §6): no test reads the real extensions dir.
/// The registry owns the loaded entries, so the tests hold it.
pub(crate) fn installed(manifest_text: &str, body: &str) -> (ActionRegistry, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = install_package(&tmp, "fixture-pkg", manifest_text);
    sh_fixture(&dir, "run.sh", body);
    let xdg = tmp.path().to_string_lossy().into_owned();
    let pairs = [("XDG_CONFIG_HOME", xdg.as_str())];
    let env = crate::telemetry::env_of(&pairs);
    let registry = ActionRegistry::load_with(&env).expect("a well-formed fixture loads");
    (registry, tmp)
}

/// The installed extension's entry, resolved the way the verb does: by
/// mount name through the registry.
pub(crate) fn extension<'a>(registry: &'a ActionRegistry, name: &str) -> &'a ExtensionEntry {
    registry
        .find(name)
        .unwrap_or_else(|| panic!("the fixture extension '{name}' must resolve"))
}

/// The fixture registry with `name` installed and `flags` declared (no
/// child body: the parse and help tests spawn nothing).
pub(crate) fn registry_with(
    name: &str,
    flags: &str,
    extra: &str,
) -> (ActionRegistry, tempfile::TempDir) {
    installed(&manifest_for(name, flags, extra), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture kit and the shipped example's copy are one text: a kit
    /// edit that lands in only one of them would ship a documented wire the
    /// fixtures no longer prove.
    #[test]
    fn the_shipped_example_carries_the_fixture_kit() {
        assert!(
            SHIPPED_EXAMPLE.contains(mcp_kit().trim_end()),
            "docs/api/extensions/examples/so-to-graph.sh no longer carries the fixture kit \
             verbatim; re-inline docs/api/extensions/examples/mcp-client.sh between the markers"
        );
    }

    /// The kit defines the two moves every fixture child makes, and the
    /// composed body puts the handshake first — the order the host reads.
    #[test]
    fn a_composed_child_body_is_the_kit_then_the_handshake() {
        let body = mcp_child_body("fixture-pkg", "mcp_call report_output '{\"text\":\"x\"}'");
        assert!(body.starts_with("# The MCP revision this kit speaks"));
        assert!(body.contains("mcp_init() {"));
        assert!(body.contains("mcp_call() {"));
        let handshake = body
            .find("mcp_init fixture-pkg 0.1.0")
            .expect("the handshake");
        let call = body.find("mcp_call report_output").expect("the body");
        assert!(handshake < call, "the handshake opens the session");
    }
}
