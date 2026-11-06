//! The bash template (`--lang sh`): a TODO-marked `walk()` plus the
//! canonical kit inlined below it — a package ships one executable
//! (ADR-0429 §2), so the kit [`crate::extensions::kit::mcp_kit`] reads is
//! pasted in rather than sourced.

use super::{render, PackageFile, Role};
use crate::extensions::kit::mcp_kit;

/// The generated executable above the inlined kit: the walk, and the one
/// call to it.
const SCRIPT: &str = r#"#!/usr/bin/env bash
# __NAME__ — a Relate extension (MCP stdio client), scaffolded by
# `relate extension init __NAME__`.
#
# Glue: the transport contract lives in __GLUE_SOURCE__. The kit below is
# `docs/api/extensions/examples/mcp-client.sh`'s body, inlined verbatim — a
# package ships one executable, so there is nothing to source and no client
# dependency to install. Re-scaffold or copy that section when the contract
# changes.
#
# Edit `walk()`; everything below it is the generated kit.
#
#   relate extension install __NAME__ --from .
#   relate run __NAME__ --name Alex -d ./store

set -euo pipefail

# The sample's two statements. `$rows` and `$name` are Cypher parameters,
# bound from the arguments object — never spliced into the statement text.
WRITE_STATEMENT='UNWIND $rows AS row
MERGE (n:Scaffold {id: row.id})
SET n.note = row.note
RETURN count(n) AS written'
GREETING="RETURN 'Hello ' + \$name + ' world!' AS msg"

# walk — put one corpus into the store. The host owns the store, the caps, the
# commit policy and the summary; this function only makes tool calls.
walk() {
    mcp_init __NAME__ 0.1.0

    # The envelope carries the manifest's resolved flags: `name` is the
    # sample's one flag (default `there`). TODO: declare the flags you need
    # in `extension.json`'s `flags` table and read them with mcp_flag_string /
    # mcp_flag_int / mcp_flag_bool / mcp_flag_list (they read the envelope
    # mcp_init consumed). Emit one `$rows` entry per record.
    name=$(mcp_flag_string name)

    # `json_string` encodes a value as a JSON string literal: the arguments
    # this kit sends are JSON text, so a flag is encoded, never spliced raw.
    written=$(mcp_call write_cypher "$(printf '{"statement":%s,"parameters":{"rows":[{"id":"example","note":%s}]}}' \
        "$(json_string "$WRITE_STATEMENT")" "$(json_string "$name")")")
    created=$(mcp_number "$written" nodes_created)
    [ -n "$created" ] || created=0

    # The flag as a Cypher parameter, and its result back: `$name` is bound
    # from the arguments object, and the read's `msg` is the greeting.
    greeting=$(mcp_string "$(mcp_call read_cypher "$(printf '{"statement":%s,"parameters":{"name":%s}}' \
        "$(json_string "$GREETING")" "$(json_string "$name")")")" msg)

    # The run's summary: the host prints it, then offers the starters to the
    # interactive shell.
    mcp_call report_output "$(printf '{"text":%s,"starters":["MATCH (n:Scaffold) RETURN n"]}' \
        "$(json_string "__NAME__: $greeting — wrote $created sample node(s)")")" >/dev/null
}

# ---------------------------------------------------------------------------
# The generated kit: docs/api/extensions/examples/mcp-client.sh, inlined
# verbatim (a package ships one executable). Nothing below is __NAME__'s.
# ---------------------------------------------------------------------------
"#;

/// The run, below the kit: bash defines functions as it reads, so the call
/// comes after the kit has defined them.
const RUN: &str = r#"
# ---------------------------------------------------------------------------
# The run: the walk over the generated kit above.
# ---------------------------------------------------------------------------
walk
"#;

/// The bash package: the executable (the walk plus the canonical kit inlined
/// below it) and the README, plus the manifest's `executable` value — the
/// executable's own path.
pub(super) fn package(name: &str) -> (String, Vec<PackageFile>) {
    let mut contents = render(SCRIPT, name);
    contents.push_str(mcp_kit());
    contents.push_str(RUN);
    let executable = PackageFile::new(format!("{name}.sh"), contents, Role::GlueExecutable);
    let readme = PackageFile::new("README.md", render(README, name), Role::Readme);
    (executable.path.clone(), vec![executable, readme])
}

/// The README body: install, run, query, then the edit loop.
const README: &str = r#"# __NAME__

A Relate extension, scaffolded by `relate extension init __NAME__
--lang sh`. The package is `extension.json` (the manifest) plus
`__NAME__.sh` (the executable: `walk()` and the inlined MCP kit) — bash and
nothing else, no client to install.

## Run it

```bash
relate extension install __NAME__ --from .
relate run __NAME__ --name Alex -d ./store
relate query --database ./store "MATCH (n:Scaffold) RETURN n.id AS id, n.note AS note"
```

`relate run` starts this process, hands it the manifest's flag values as
one JSON envelope line on stdin, and from then on the two pipes carry
JSON-RPC 2.0: this script is the MCP client, the host is the MCP server that
owns the store. The run ends with the summary `walk()` reports.

## Edit it

`walk()` is the only part you write. Everything below it is the canonical
kit (`docs/api/extensions/examples/mcp-client.sh`, inlined because a package
ships one executable); its source of truth is __GLUE_SOURCE__.

1. The `flags` table already declares the sample's one flag — `--name`
   (default `there`), which `walk()` reads with `mcp_flag_string` and binds as
   the `$name` Cypher parameter. Add your own flags there (`string`,
   `integer`, `boolean`, `repeatable`) and read them the same way.
2. Replace the sample row with your own records and shape the graph in the
   `write_cypher` statement's `UNWIND`.
3. `mcp_call report_output` is the run's summary; its starters are offered
   to the interactive shell.

The author guide — the `init → edit → check → install --from → run` loop,
the language tiers and the transport contract — is
`docs/api/extensions/authoring.md`.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// The kit rides inline, past the walk, in the one canonical text: the
    /// shipped body, verbatim, after the walk it serves.
    #[test]
    fn the_bash_glue_inlines_the_canonical_kit_below_the_walk() {
        let (executable, files) = package("my-ext");
        assert_eq!(executable, "my-ext.sh");
        let file = &files[0];
        assert_eq!(file.role, Role::GlueExecutable);
        assert!(file.contents.starts_with("#!/usr/bin/env bash\n"));
        assert!(
            file.contents.contains(mcp_kit().trim_end()),
            "the kit is inlined verbatim"
        );
        let walk = file.contents.find("walk() {").expect("the walk");
        let kit = file
            .contents
            .find("# The MCP revision this kit speaks")
            .expect("the kit");
        assert!(walk < kit, "the walk comes first, the kit is the tail");
    }
}
