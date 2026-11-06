//! The TypeScript template (`--lang ts`): the transport (a line reader over
//! the process's own stdin) and the minimal MCP client as generated source,
//! then the TODO-marked `walk()`. No SDK, no `package.json`, no dependency:
//! after Node's own type strip the file is plain JavaScript.

use super::{render, PackageFile, Role};

/// The generated TypeScript executable — transport, client, sample walk.
const TS: &str = r#"#!/usr/bin/env node
/**
 * __NAME__ — a Relate extension (MCP stdio client).
 *
 * Scaffolded by `relate extension init __NAME__`. The package is
 * `extension.json` (the manifest the host parses — mount name, MCP revision,
 * flag table, commit policy) plus this executable (the file the host
 * spawns); `README.md` is the quickstart and is not part of the install.
 *
 * Glue: the transport contract lives in __GLUE_SOURCE__. The line reader and
 * the minimal client below are generated inline — no published client
 * dependency to install or manage: re-scaffold with `relate extension
 * init` or copy that section when the contract changes. No imports either:
 * after Node's type strip this file is plain CommonJS-compatible JavaScript,
 * so `node` runs it as it is (Node 22.18+ strips the types).
 *
 * Edit `walk()`; everything above it is the generated glue.
 */

type Json = Record<string, unknown>;

/** The MCP revision this package speaks — the manifest's `protocol`. */
const PROTOCOL_VERSION = "__PROTOCOL_VERSION__";
const NAME = "__NAME__";
const VERSION = "0.1.0";

/** A JSON-RPC 2.0 message, the fields the client reads. */
interface RpcMessage {
  id?: number;
  result?: Json;
  error?: { code: number; message: string };
}

/** The host's messages, one JSON line each, read from the process's stdin. */
class Lines {
  private buffer = "";
  private queue: string[] = [];
  private waiters: ((line: string) => void)[] = [];

  constructor() {
    process.stdin.on("data", (chunk: Buffer) => {
      this.buffer += chunk.toString("utf8");
      for (let at = this.buffer.indexOf("\n"); at >= 0; at = this.buffer.indexOf("\n")) {
        const line = this.buffer.slice(0, at);
        this.buffer = this.buffer.slice(at + 1);
        const waiter = this.waiters.shift();
        if (waiter !== undefined) waiter(line);
        else this.queue.push(line);
      }
    });
    process.stdin.resume();
  }

  next(): Promise<string> {
    const queued = this.queue.shift();
    if (queued !== undefined) return Promise.resolve(queued);
    return new Promise((resolve) => this.waiters.push(resolve));
  }
}

/**
 * The minimal MCP client: one JSON-RPC line per message, ids echoed. The
 * host is the MCP server — it alone owns the store — so every call below is
 * a `tools/call` on its five-tool table. The handshake publishes the mount's
 * caps and the store's capacity; both stay on the client after `start()`.
 */
class Client {
  private lines = new Lines();
  private nextId = 0;
  caps: Json = {};
  capacity: Json = {};

  private send(message: Json): void {
    process.stdout.write(JSON.stringify(message) + "\n");
  }

  private async request(method: string, params: Json): Promise<Json> {
    const id = ++this.nextId;
    this.send({ jsonrpc: "2.0", id, method, params });
    for (;;) {
      const message = JSON.parse(await this.lines.next()) as RpcMessage;
      if (message.id !== id) continue; // a notification, or an answer nobody asked for
      if (message.error !== undefined) {
        throw new Error(`${method} refused: ${JSON.stringify(message.error)}`);
      }
      return message.result ?? {};
    }
  }

  /** Read the envelope, run the initialize handshake, confirm it. */
  async start(): Promise<Json> {
    const envelope = JSON.parse(await this.lines.next()) as Json;
    const result = await this.request("initialize", {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: { name: NAME, version: VERSION },
    });
    if (result.protocolVersion !== PROTOCOL_VERSION) {
      throw new Error(`the host speaks ${String(result.protocolVersion)}, not ${PROTOCOL_VERSION}`);
    }
    this.caps = (result.caps ?? {}) as Json;
    this.capacity = (result.capacity ?? {}) as Json;
    this.send({ jsonrpc: "2.0", method: "notifications/initialized" });
    return envelope;
  }

  /** One `tools/call`; returns the tool's JSON result. */
  async call(tool: string, args: Json): Promise<Json> {
    const result = await this.request("tools/call", { name: tool, arguments: args });
    const blocks = (result.content ?? []) as { type: string; text?: string }[];
    for (const block of blocks) {
      if (block.type === "text" && block.text !== undefined) {
        return JSON.parse(block.text) as Json;
      }
    }
    throw new Error(`'${tool}' answered no text content`);
  }
}

const WRITE = `UNWIND $rows AS row
MERGE (n:Scaffold {id: row.id})
SET n.note = row.note
RETURN count(n) AS written`;

const READ = "MATCH (n:Scaffold) RETURN count(n) AS nodes";

// The sample's greeting: `$name` is a Cypher parameter, bound from the
// arguments object — never spliced into the statement text.
const GREETING = "RETURN 'Hello ' + $name + ' world!' AS msg";

/** The first row's `field`, as a string — the read results' one accessor. */
function rowField(result: Json, field: string): string {
  // A read result's rows are objects keyed by the statement's RETURN names.
  const rows = (result.rows ?? []) as Json[];
  return rows.length > 0 ? String(rows[0][field] ?? "") : "";
}

/**
 * Walk one corpus into the store — the part you write.
 *
 * TODO: replace the sample record with your own source. Declare your flags
 * in `extension.json`'s `flags` table first, read them from `envelope` (the
 * host resolves every flag's default — `name` here is the sample's one
 * flag), emit one row object per record, and shape the graph in `WRITE`.
 * `docs/api/extensions/authoring.md` walks the loop.
 */
async function walk(client: Client, envelope: Json): Promise<void> {
  const name = typeof envelope.name === "string" ? envelope.name : "";
  const rows = [{ id: "example", note: name }];
  const written = await client.call("write_cypher", { statement: WRITE, parameters: { rows } });
  const counts = (written.counts ?? {}) as Json;
  const found = await client.call("read_cypher", { statement: READ });
  const greeted = await client.call("read_cypher", {
    statement: GREETING,
    parameters: { name },
  });
  const nodes = rowField(found, "nodes") || "0";
  const msg = rowField(greeted, "msg");
  await client.call("report_output", {
    text: `${NAME}: ${msg} — wrote ${String(counts.nodes_created ?? 0)} sample node(s), the store holds ${nodes}`,
    starters: ["MATCH (n:Scaffold) RETURN n"],
  });
}

async function main(): Promise<void> {
  const client = new Client();
  await walk(client, await client.start());
}

main().then(
  // The session is over: exit. The stdin handle the line reader holds would
  // otherwise keep the process alive while the host waits for requests that
  // never come, and only its read deadline would end the run.
  () => process.exit(0),
  (error: unknown) => {
    process.stderr.write(`__NAME__: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exit(1);
  },
);
"#;

/// The TypeScript package: the executable (the generated glue) and the
/// README, plus the manifest's `executable` value — the executable's own
/// path.
pub(super) fn package(name: &str) -> (String, Vec<PackageFile>) {
    let executable = PackageFile::new(format!("{name}.ts"), render(TS, name), Role::GlueExecutable);
    let readme = PackageFile::new("README.md", render(README, name), Role::Readme);
    (executable.path.clone(), vec![executable, readme])
}

/// The README body: install, run, query, then the edit loop.
const README: &str = r#"# __NAME__

A Relate extension, scaffolded by `relate extension init __NAME__
--lang ts`. The package is `extension.json` (the manifest) plus `__NAME__.ts`
(the executable: a line transport, a minimal MCP client and `walk()`) — no
`package.json`, no `node_modules`, nothing to install, because the glue is
generated inline.

## Run it

Needs Node 22.18+ (it strips the type annotations; there is no build step).

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

`walk()` is the only part you write. Everything above it — the line reader,
the handshake, `tools/call` — is generated glue whose source of truth is
__GLUE_SOURCE__.

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

    /// The TypeScript glue carries no import and no SDK: after Node's type
    /// strip it is plain JavaScript, so the spawn needs nothing but `node`.
    #[test]
    fn the_typescript_glue_imports_nothing_and_speaks_this_hosts_revision() {
        let (executable, files) = package("my-ext");
        assert_eq!(executable, "my-ext.ts");
        let file = &files[0];
        assert_eq!(file.role, Role::GlueExecutable);
        assert!(file.contents.starts_with("#!/usr/bin/env node\n"));
        assert!(
            !file.contents.contains("import "),
            "no dependency to install"
        );
        assert!(!file.contents.contains("@modelcontextprotocol"));
        assert!(file.contents.contains(&format!(
            "const PROTOCOL_VERSION = \"{MCP_PROTOCOL_VERSION}\";"
        )));
        assert!(file.contents.contains("async function walk("));
    }
}
