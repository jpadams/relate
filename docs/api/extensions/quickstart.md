# Extensions — quickstart

Fifteen minutes from an empty extensions directory to a graph, through the
MCP stdio mount: scaffold a package, edit one function, install it with
`extension install --from`, run it, read the result back.

```text
init → edit → check → install --from → run
```

## 1. Scaffold it

```bash
relate extension init word-count            # --lang py is the default
ls word-count
# extension.json  README.md  word-count.py
```

The scaffold is a complete package: the manifest the host parses, an
executable that is a minimal MCP client (the transport generated inline — no
client dependency to install), a TODO-marked `walk()` and a README
quickstart. `--lang sh|py|ts|rust` picks the language; every other language
reaches the same wire by construction (see the [author guide](authoring.md)).

Edit `walk()` — the one function that says what your extension *does* — then
check the wiring without installing anything:

```bash
relate extension check ./word-count
```

and install it:

```bash
relate extension install word-count --from ./word-count
relate run word-count --name Alex -d ./store
# word-count: Hello Alex world! — wrote 1 sample node(s), the store holds 1
relate query --database ./store "MATCH (n:Scaffold) RETURN n.id AS id"
```

The `--name Alex` is the sample's own flag: it shows the round trip every
extension needs — the manifest's `flags` table declares it, the host resolves
its default into the request envelope, `walk()` binds it as the `$name`
Cypher parameter, and the read's result is the summary line.

That is the whole loop, and its reference is the [author guide](authoring.md).
The rest of this page is the *other* starting point — the shipped hello-world
example, a real walk of the live Stack Exchange API, which is worth reading
once you have the loop.

The shipped example fetches the latest Stack Overflow questions for one tag
from the public Stack Exchange API and loads them with the
corpus's own schema and write shape — one
`update_schema` for the graph type and its promotion, one `batch_cypher` call
with the two load statements, one `read_cypher` verification and one
`report_output` — and it ships in three languages, so you can start from the
one you already have a runtime for.

All three need **network access** (the live API) and a **fresh store** (the
promoted graph type refuses a re-import of the same questions).

The files are in [`examples/`](examples/):

| Example | Runtime to install | Package |
| --- | --- | --- |
| [`so-to-graph.sh`](examples/so-to-graph.sh) | `curl` and `jq` (the wire kit, [`mcp-client.sh`](examples/mcp-client.sh), is inlined) | `extension.bash.json` |
| [`so-to-graph.py`](examples/so-to-graph.py) | `pip install 'mcp>=2'` (the MCP SDK 2.x) | `extension.python.json` |
| [`so-to-graph.ts`](examples/so-to-graph.ts) | `npm install @modelcontextprotocol/client`, Node 22.18+ | `extension.typescript.json` + `package.typescript.json` |

All three walk the same live corpus with the same flags — `--tag` (default
`neo4j`), `--pages` (API pages of 100, default 1), `--body` (write the API's
markdown body) — so the numbers below hold for any of them (they move with
the API's newest questions).

## What an extension package is

A package is a directory holding `extension.json` and one executable. That
is the whole contract for `extension install --from`: it copies the
manifest and the executable, records the provenance, and nothing else
rides ([spec.md](spec.md), "Verbs and lifecycle"). The manifest declares
the mount name, the MCP revision (`protocol` — the child is the MCP
client), the commit policy, and the flag table the host parses argv from:

```json
{
  "kind": "command",
  "name": "so-to-graph",
  "version": "0.1.0",
  "protocol": "2025-06-18",
  "flags": [
    {"kind": "string", "name": "tag", "default": "neo4j"},
    {"kind": "integer", "name": "pages", "default": 1},
    {"kind": "boolean", "name": "body", "default": false}
  ],
  "commit": "per-batch",
  "executable": "so-to-graph.sh"
}
```

## 2. Copy the template

From a checkout of this repository:

```bash
mkdir -p /tmp/so-to-graph
cp docs/api/extensions/examples/so-to-graph.sh /tmp/so-to-graph/
cp docs/api/extensions/examples/extension.bash.json /tmp/so-to-graph/extension.json
chmod +x /tmp/so-to-graph/so-to-graph.sh
```

The manifest is renamed to `extension.json` — the one name the installer
and the discovery walk read. The executable keeps the name its `executable`
field declares; it is self-contained, because a package is one executable
(the example's kit, [`mcp-client.sh`](examples/mcp-client.sh), is inlined
into it, and `scripts/check-extension-examples.sh` pins the two).

## 3. Install it

```bash
relate extension install so-to-graph --from /tmp/so-to-graph
```

```text
installed local extension so-to-graph 0.1.0 from /tmp/so-to-graph
```

`--from` is the offline, local-directory trust act: whoever provisions the
directory owns what is in it. The other tiers — the signed first-party
channel, a GitHub commit pin, an unsigned https URL — and what the loud
install line and the provenance marker mean are the
[install guide](install.md). The name in `extension install <name>` must
match the manifest's `name`. Check the install landed:

```bash
relate extension list
```

```text
so-to-graph 0.1.0 local /tmp/so-to-graph
```

## 4. Run it

```bash
relate run so-to-graph --tag neo4j --pages 1 --body -d ./so
```

```text
imported 100 questions, 99 answers, 96 tags and 122 users tagged 'neo4j' with bodies
  MATCH (t1:Tag)<-[:TAGGED]-(q:Question)-[:TAGGED]->(t2:Tag) WHERE t1.name < t2.name RETURN t1.name AS tag, t2.name AS co_occurs, count(q) AS questions ORDER BY questions DESC, tag, co_occurs LIMIT 10
  MATCH (u:User)-[:PROVIDED]->(:Answer)-[:ANSWERED]->(:Question)-[:TAGGED]->(t:Tag) RETURN u.display_name AS user, count(*) AS answers ORDER BY answers DESC, user LIMIT 10
```

(The counts are one page of the API's newest `neo4j` questions on the day
it ran; they move as questions arrive.)

The first line is the child's `report_output`; the two indented lines are
its starters, which the interactive handoff offers when the run opens a
shell (no `-d` on a terminal). The manifest's flag table is the action's
whole grammar — `relate run so-to-graph --help` prints it, defaults and
all.

The run takes a second or two: one API page, then one transaction for the
load. The host owns the store, the transaction framing and the caps; the
child only fetches and calls tools.

## 5. Read the graph back

```bash
relate query --database ./so "MATCH (q:Question)-[:TAGGED]->(t:Tag) RETURN t.name AS tag, count(q) AS questions ORDER BY questions DESC, tag LIMIT 10"
```

```text
{"tag":"neo4j","questions":100}
{"tag":"cypher","questions":61}
{"tag":"python","questions":8}
…
```

What the tools built, one line each:

| Tool | What the example asked for | What happened |
| --- | --- | --- |
| `update_schema` | the `SOGraph` graph type, then its promotion | The type and its derived constraints; a duplicate key is then refused |
| `batch_cypher` | the questions statement and the answers statement, one row set each | Both statements in the one transaction the call gives |
| `write_cypher` | one duplicate question key | Refused by the promoted key constraint — the schema step's assertion |
| `read_cypher` | questions, answers, tags and users | The verification pass; a mismatch fails the run |
| `report_output` | the summary line and the two starters | What the host printed above |

The flags are the manifest's, resolved by the host into the request
envelope the child reads first: `--tag` names the Stack Overflow tag,
`--pages` how many API pages (100 questions each) to walk, and `--body` the
API markdown body onto the nodes.

## The Python example

Same flow, one runtime to install first:

```bash
pip install 'mcp>=2'

mkdir -p /tmp/so-to-graph-py
cp docs/api/extensions/examples/so-to-graph.py /tmp/so-to-graph-py/
cp docs/api/extensions/examples/extension.python.json /tmp/so-to-graph-py/extension.json
chmod +x /tmp/so-to-graph-py/so-to-graph.py

relate extension install so-to-graph-py --from /tmp/so-to-graph-py
relate run so-to-graph-py --tag neo4j --pages 1 -d ./so-py
```

The session is the official MCP Python SDK's `mcp.Client` (SDK **2.x**):
the example implements the SDK's `Transport` protocol over the two pipes it
was handed (the SDK's stock stdio transport spawns the server; here the
host spawned *us*), and everything after that — the handshake, the JSON-RPC
framing, `tools/call`, the typed results — is SDK code. The client is
created with `mode="legacy"` because this host serves the handshake-era
protocol, not the SDK's modern `server/discover` probe. Requires Python
3.11+ (`mcp` 2.x) and the standard MCP handshake in the host's
`initialize` result ([spec.md](spec.md), "Handshake").

## The TypeScript example

Node needs the SDK in the package directory, and the package directory
therefore carries more than the two accounted files — so it is placed by
hand rather than installed with `--from`:

```bash
pkg=/tmp/so-to-graph-ts
mkdir -p "$pkg"
cp docs/api/extensions/examples/so-to-graph.ts "$pkg/"
cp docs/api/extensions/examples/extension.typescript.json "$pkg/extension.json"
cp docs/api/extensions/examples/package.typescript.json "$pkg/package.json"
(cd "$pkg" && npm install)
chmod +x "$pkg/so-to-graph.ts"

ext="${XDG_CONFIG_HOME:-$HOME/.config}/relate/extensions"
mkdir -p "$ext/so-to-graph-ts"
cp -R "$pkg/." "$ext/so-to-graph-ts/"

relate run so-to-graph-ts --tag neo4j --pages 1 -d ./so-ts
```

`package.typescript.json` declares `"type": "module"` (the file is ESM)
and the SDK dependency (`@modelcontextprotocol/client` **2.x**). The
extension is the official SDK's `Client` over a `Transport` implemented on
the pipes the host handed it, framed with the SDK's own `ReadBuffer` and
`serializeMessage` — the same `start`/`send`/`onclose` contract the SDK's
`StdioClientTransport` implements around a spawned process. Like the Python
example it needs the standard MCP handshake in the host's `initialize`
result ([spec.md](spec.md), "Handshake").

## What fails closed

- An unknown name never fetches anything: exit 2, the pinned discovery
  error naming the install path.
- A package whose manifest does not decode, whose name collides, or whose
  executable escapes its directory fails the install and the registry load
  — never a skipped package.
- A child that fails a tool call, exceeds a cap, or exits non-zero fails
  the run; under `whole-import` the session's transaction rolls back, so a
  failed run never leaves a committed store.

Full contract: [spec.md](spec.md) — the manifest schema, the mount's wire
and caps, the [Transport contract](spec.md#the-transport-contract), host
execution, and the error classes. The authoring loop is the
[author guide](authoring.md), the install side the [install guide](install.md),
and the protocol walkthrough from a child's side
[examples/run.md](examples/run.md); the decisions are
[ADR-0442](../../adr/0442-extension-mcp-stdio-wire.md) (the MCP mount) and
[ADR-0447](../../adr/0447-extension-single-wire.md) (one wire, one tool
table).
