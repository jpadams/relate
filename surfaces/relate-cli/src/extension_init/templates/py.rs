//! The Python template (the default `--lang py`): the generated transport
//! and minimal MCP client as three stdlib-only classes/functions, then the
//! TODO-marked `walk()`. No `mcp` SDK, no package to install — the glue is
//! the package.

use super::{render, PackageFile, Role};

/// The generated Python executable — glue, a sample `walk()`, and nothing
/// to install.
const PY: &str = r#"#!/usr/bin/env python3
"""__NAME__ — a Relate extension (MCP stdio client).

Scaffolded by `relate extension init __NAME__`. The package is
`extension.json` (the manifest the host parses — mount name, MCP revision,
flag table, commit policy) plus this executable (the file the host spawns);
`README.md` is the quickstart and is not part of the install.

Glue: the transport contract lives in __GLUE_SOURCE__. The transport and
the minimal client below are generated inline — no published client
dependency to install or manage: re-scaffold with `relate extension init`
or copy that section when the contract changes.

Edit `walk()`; everything above it is the generated glue. Standard library
only: python3 is the whole runtime.
"""

from __future__ import annotations

import json
import sys
from typing import Any

# The MCP revision this package speaks — the manifest's `protocol` and the
# handshake's `protocolVersion`. A host that answers another revision fails
# the handshake instead of downgrading.
PROTOCOL_VERSION = "__PROTOCOL_VERSION__"
NAME = "__NAME__"
VERSION = "0.1.0"


def read_envelope() -> dict[str, Any]:
    """The host's request envelope: the manifest's resolved flag values, one
    JSON line on stdin, written before the session starts."""
    line = sys.stdin.readline()
    if not line:
        raise SystemExit("mcp: the host wrote no request envelope")
    return json.loads(line)


class Client:
    """The minimal MCP client: one JSON-RPC line per message, ids echoed.

    The host is the MCP server — it alone owns the store — so every call
    below is a `tools/call` on its five-tool table. The handshake publishes
    the mount's caps and the store's capacity; both stay on the client after
    `start()` so a walk can size its work against them.
    """

    def __init__(self) -> None:
        self._next_id = 0
        self.caps: dict[str, Any] = {}
        self.capacity: dict[str, Any] = {}

    def _send(self, message: dict[str, Any]) -> None:
        sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
        sys.stdout.flush()

    def request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        self._next_id += 1
        request_id = self._next_id
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        while True:
            line = sys.stdin.readline()
            if not line:
                raise SystemExit(f"mcp: the host closed the session before answering {method}")
            message = json.loads(line)
            if message.get("id") != request_id:
                continue  # a notification, or an answer nobody asked for
            if "error" in message:
                raise SystemExit(f"mcp: {method} refused: {json.dumps(message['error'])}")
            return message.get("result", {})

    def start(self) -> dict[str, Any]:
        """Read the envelope, run the initialize handshake, confirm it."""
        envelope = read_envelope()
        result = self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": NAME, "version": VERSION},
            },
        )
        answered = result.get("protocolVersion")
        if answered != PROTOCOL_VERSION:
            raise SystemExit(f"mcp: the host speaks {answered!r}, not {PROTOCOL_VERSION!r}")
        self.caps = result.get("caps", {})
        self.capacity = result.get("capacity", {})
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        return envelope

    def call(self, tool: str, arguments: dict[str, Any]) -> dict[str, Any]:
        """One `tools/call`; returns the tool's JSON result."""
        result = self.request("tools/call", {"name": tool, "arguments": arguments})
        for block in result.get("content", []):
            if block.get("type") == "text":
                return json.loads(block["text"])
        raise SystemExit(f"mcp: '{tool}' answered no text content")


WRITE = """UNWIND $rows AS row
MERGE (n:Scaffold {id: row.id})
SET n.note = row.note
RETURN count(n) AS written"""

READ = "MATCH (n:Scaffold) RETURN count(n) AS nodes"

# The sample's greeting: `$name` is a Cypher parameter, bound from the
# arguments object — never spliced into the statement text.
GREETING = "RETURN 'Hello ' + $name + ' world!' AS msg"


def walk(client: Client, envelope: dict[str, Any]) -> None:
    """Walk one corpus into the store — the part you write.

    TODO: replace the sample record with your own source. Declare your flags
    in `extension.json`'s `flags` table first, read them from `envelope`
    (the host resolves every flag's default — `name` here is the sample's
    one flag), emit one row dict per record, and shape the graph in
    `WRITE`. `docs/api/extensions/authoring.md` walks the loop.
    """
    name = envelope["name"]
    rows = [{"id": "example", "note": name}]
    written = client.call("write_cypher", {"statement": WRITE, "parameters": {"rows": rows}})
    found = client.call("read_cypher", {"statement": READ})
    greeted = client.call(
        "read_cypher", {"statement": GREETING, "parameters": {"name": name}}
    )
    created = written.get("counts", {}).get("nodes_created", 0)
    # A read result's rows are objects keyed by the statement's RETURN names.
    nodes = found["rows"][0].get("nodes", 0) if found.get("rows") else 0
    msg = greeted["rows"][0].get("msg", "") if greeted.get("rows") else ""
    client.call(
        "report_output",
        {
            "text": f"{NAME}: {msg} — wrote {created} sample node(s), the store holds {nodes}",
            "starters": ["MATCH (n:Scaffold) RETURN n"],
        },
    )


def main() -> None:
    client = Client()
    walk(client, client.start())


if __name__ == "__main__":
    main()
"#;

/// The Python package: the executable (the generated glue) and the README,
/// plus the manifest's `executable` value — the executable's own path.
pub(super) fn package(name: &str) -> (String, Vec<PackageFile>) {
    let executable = PackageFile::new(format!("{name}.py"), render(PY, name), Role::GlueExecutable);
    let readme = PackageFile::new("README.md", render(README, name), Role::Readme);
    (executable.path.clone(), vec![executable, readme])
}

/// The README body: install, run, query, then the edit loop.
const README: &str = r#"# __NAME__

A Relate extension, scaffolded by `relate extension init __NAME__`.
The package is `extension.json` (the manifest) plus `__NAME__.py` (the
executable — a minimal MCP client with the transport generated inline, so
there is nothing to `pip install`).

## Run it

```bash
relate extension install __NAME__ --from .
relate run __NAME__ --name Alex -d ./store
relate query --database ./store "MATCH (n:Scaffold) RETURN n.id AS id, n.note AS note"
```

`relate run` starts this process, hands it the manifest's flag values as
one JSON envelope line on stdin, and from then on the two pipes carry
JSON-RPC 2.0: this file is the MCP client, the host is the MCP server that
owns the store. The run ends with the summary `walk()` reports.

## Edit it

`walk()` is the only part you write. Everything above it — the envelope
read, the handshake, `tools/call` — is generated glue whose source of truth
is __GLUE_SOURCE__.

1. The `flags` table already declares the sample's one flag — `--name`
   (default `there`), which `walk()` reads from `envelope` and binds as the
   `$name` Cypher parameter. Add your own flags there (`string`, `integer`,
   `boolean`, `repeatable`) and read them the same way.
2. Replace the sample row with your own records and shape the graph in
   `WRITE` (one statement, `$rows` is the row list).
3. `report_output` is the run's summary; its starters are offered to the
   interactive shell.

The author guide — the `init → edit → check → install --from → run` loop,
the language tiers and the transport contract — is
`docs/api/extensions/authoring.md`.
"#;

#[cfg(test)]
mod tests {
    use super::super::MCP_PROTOCOL_VERSION;
    use super::*;

    /// The Python glue: the interpreter's shebang, the revision rendered from
    /// the wire constant, and a `walk()` the author edits.
    #[test]
    fn the_python_glue_speaks_this_hosts_revision() {
        let (executable, files) = package("my-ext");
        assert_eq!(executable, "my-ext.py");
        let file = &files[0];
        assert_eq!(file.path, "my-ext.py");
        assert_eq!(file.role, Role::GlueExecutable);
        assert!(file.contents.starts_with("#!/usr/bin/env python3\n"));
        assert!(file
            .contents
            .contains(&format!("PROTOCOL_VERSION = \"{MCP_PROTOCOL_VERSION}\"")));
        assert!(file.contents.contains("def walk("));
        assert!(!file.contents.contains("import mcp"), "no SDK to install");
    }
}
