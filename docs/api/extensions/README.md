# Extensions

The extension system is how userland commands reach the CLI: a package
that walks some source and writes the graph through MCP tool calls, while
the host alone owns the store, the caps, the commit policy and the exit
codes ([description](description.md)).

One page per question:

| You want to… | Read |
| --- | --- |
| Get running in fifteen minutes | [quickstart.md](quickstart.md) — `init → edit → check → install --from → run` on a scaffold |
| Author a package | [authoring.md](authoring.md) — the scaffold's files, the four first-class languages, the `check` wiring probe |
| Read the exact contract | [spec.md](spec.md) — manifest schema, discovery, the mount's MCP stdio wire, caps, commit policies, error classes |
| Install or distribute one | [install.md](install.md) — the trust tiers (first-party / GitHub pin / unsigned URL / local directory) |
| See real packages | [examples/](examples/) — `so-to-graph.sh`/`.py`/`.ts`, the same live Stack Exchange walk in three toolchains, plus [run.md](examples/run.md) and [mcp-mount.md](examples/mcp-mount.md) |

## The shipped extensions

Two first-party packages, extension crates speaking the same MCP stdio
wire (ADR-0431 §11):

```text
$ relate extension install fs-to-graph     # first-party channel
$ relate extension install git-to-graph
$ relate run git-to-graph --help           # the flag grammar is the manifest's own
```

| Extension | What it walks |
| --- | --- |
| `fs-to-graph` | a directory tree → the graph ([ADR-0157](../../adr/0157-fs-to-graph-import.md)) |
| `git-to-graph` | a checkout's history → the graph ([ADR-0376](../../adr/0376-git-to-graph-import.md)) |

The MCP surface an extension's tools speak is the product server's own
([MCP](../mcp/spec.md)) — one tool table, both surfaces.
