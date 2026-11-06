# Extensions — author guide

An extension is a package that walks some source and writes the graph
through MCP tool calls; the host owns the store, the caps, the commit policy
and the exit codes. This guide is the author's loop:

```
init → edit → check → install --from → run
```

The exact contract behind each step is [spec.md](spec.md); the trust tiers
of the install are [install.md](install.md).

## 1. Scaffold

```bash
relate extension init word-count            # --lang py is the default
relate extension init word-count --lang sh
```

`init` writes one new directory `<name>/` in the working directory:

| File | What it is |
| --- | --- |
| `extension.json` | The manifest the host parses: mount name, `protocol` (this host's MCP revision), the flag table, the commit policy, the executable |
| `<name>.py` / `.sh` / `.ts` (Rust: `Cargo.toml` + `src/main.rs`) | The executable: the generated MCP client glue, then a TODO-marked `walk()` |
| `README.md` | The quickstart for that language |

The directory is created fresh. An existing one is a refusal (exit 1) —
`init` never merges into or clobbers an author's files — and a failed
scaffold removes what it created, so the working directory is left as it
was found. The name follows the installer's one package-name rule — a
single directory segment (not empty, not `.`/`..`, no path separator) — so a
scaffold is never a directory `extension install` would refuse, and it must
not be a reserved command word (`run`, `extension`, `query`, …); a bad name,
an unknown `--lang` or a missing name exits 2.

## 2. Edit

`walk()` is the only part you write. Everything around it is generated glue
whose source of truth is [spec.md, "The Transport contract"](spec.md#the-transport-contract)
— the generated files say so in their header, so updating the glue means
re-scaffolding or copying that section, never tracking a client package
version.

The scaffold's manifest declares one flag (`--name`, default `there`) and
the `per-batch` commit policy. The generated `walk()` uses that flag as its
worked round trip — it reads the value the host resolved into the request
envelope, binds it as a `$name` Cypher parameter (never spliced into the
statement), and prints the read's result in the summary — so the contract an
author has to get right is already demonstrated. Grow the table as the walk
grows:

```json
{
  "kind": "command",
  "name": "word-count",
  "version": "0.1.0",
  "protocol": "2025-06-18",
  "flags": [
    {"kind": "string", "name": "glob", "default": "**/*.md"},
    {"kind": "integer", "name": "min-length", "default": 3}
  ],
  "positional": {"name": "DIR"},
  "commit": "per-batch",
  "executable": "word-count.py"
}
```

- Every flag you declare arrives in the `walk()`'s request envelope, already
  resolved (its declared default when absent) — the child never sees raw
  argv, the store path or any other host flag.
- `commit: "per-batch"` commits each writing call, so `storeRemainingBytes`
  moves per call and a failed run keeps what already landed;
  `"whole-import"` frames the whole session in one transaction that rolls
  back on any failure. Pick the one your source's size argues for.
- The five tools (`write_cypher`, `read_cypher`, `update_schema`,
  `batch_cypher`, `report_output`) and their argument shapes are
  [spec.md, "The five tools"](spec.md#the-five-tools). `read_cypher` only
  accepts statements the plan classifies as reads and `update_schema` only
  DDL — the guards are the plan's own type, so a refusal is answered, never
  a hang.
- The mount bounds only the handshake: it waits ten seconds for the
  child's first request (`initialize`). A child that never writes anything
  fails the run with `the extension wrote no MCP request within 10s`. Once
  the first request has arrived, reads are unbounded — a child doing long
  work before its first tool call (a whole tree walk, a full `git log`) is
  not killed. Streaming rows as you collect them is still the recommended
  pattern: it keeps progress visible and memory bounded.

## 3. Check

```bash
relate extension check ./word-count
```

`check` runs the wiring the run would hit, one line per stage and without
touching a store: the manifest decodes (every load refusal), the executable
exists and is executable, the child spawns, `initialize` answers this host's
revision, `notifications/initialized` is accepted, and `tools/list` returns
the shared tool table. Usage and manifest problems exit 2, runtime and
handshake problems exit 1, success 0. It is the fastest loop: no install, no
store, one line per stage. The stages, the pin line that closes a clean run
and the exit-code split are [the check verb](spec.md#the-check-verb).

### Worked example: a check that fails, and its fix

A scaffolded package whose executable lost its exec bit (a `cp` without
`-p`, an archive round trip, a checkout that dropped the mode) stops at
the second stage:

```console
$ relate extension check ./greet
manifest: ok (greet 0.1.0, protocol 2025-06-18)
executable: failed: the extension executable ./greet/run.sh is not executable
$ echo $?
2
```

The manifest is fine; the file is not runnable. Fix the mode and re-run —
the whole contract passes:

```console
$ chmod +x ./greet/run.sh
$ relate extension check ./greet
manifest: ok (greet 0.1.0, protocol 2025-06-18)
executable: ok (run.sh)
spawn: ok
initialize: ok (MCP 2025-06-18, server relate 0.13.0)
notifications/initialized: ok
tools/list: ok (the shared tool table: 5 tools; the extension calls 'report_output')
pin: not from a GitHub source (unmarked)
$ echo $?
0
```

Each failure names the stage and the refusal that produced it, so the fix
is usually one of: correct the manifest field it names, make the file
executable, or fix the child's handshake (it must send `initialize` first,
name `2025-06-18`, then `notifications/initialized`, then only tools the
table carries — `write_cypher`, `read_cypher`, `update_schema`,
`batch_cypher`, `report_output`).

## 4. Install it locally

```bash
relate extension install word-count --from ./word-count
relate extension list                    # word-count 0.1.0 local ./word-count
```

`--from` is the local trust act: the directory you hand it is the trust
decision, nothing is fetched, nothing is signed. The installer copies
exactly two files — the manifest and the executable — records the
provenance marker, and leaves the rest of your directory (a Rust `target/`,
`.venv`, `node_modules`) behind. That is also the reason a package's
executable must be self-contained. The other tiers are [install.md](install.md).

## 5. Run it

```bash
relate run word-count docs/ -d ./store
relate query --database ./store "MATCH (n:Word) RETURN count(n) AS words"
```

The host opens the store, runs the fresh-store check when the manifest asks
for one, spawns the executable over std pipes (no shell, no argv), writes the
envelope, and answers the child's MCP requests until the child ends its
stdout. The child's `report_output` is the summary line, its starters are
offered to the interactive shell, and the run exits after the summary when
`-d` names a store (a `:memory:` run on a TTY hands the graph to the shell).

Exit codes: `0` success, `2` usage (a manifest, flag or resolution problem
before anything ran), `1` runtime (a protocol violation, a cap, a non-zero
child exit, a fresh-store refusal). Per-call refusals are answered as
JSON-RPC errors — they do not fail the run.

## Language tiers

| Tier | Languages | What you get |
| --- | --- | --- |
| **First-class** — `init --lang` | `sh`, `py`, `ts`, `rust` | The generated glue: envelope read, JSON-RPC transport, a minimal client (Rust: the wire kit's usage as source), the walk, a README — no published client dependency to install or manage |
| **Supported by construction** | `go`, `java`, `csharp`, `swift`, `lua`, `elixir`, `ruby` | Any MCP client: an official SDK's `Client` over a `Transport` implemented on the pipes you were handed (the SDKs' stock stdio transports spawn a server; here the host spawned *us*), or the raw JSON-RPC path of the [five moves](spec.md#the-transport-contract) |

Runtimes the generated templates need: `bash` (`sh`), `python3` (`py`),
Node 22.18+ (`ts`, it strips the type annotations natively), a Rust
toolchain (`rust`; the crate is laid out as a first-party child, so its
dependencies come from the workspace).

## Worked example: from scaffold to graph

```bash
relate extension init word-count
$EDITOR word-count/word-count.py                  # walk() reads ./docs, writes (:Word)
relate extension check ./word-count
relate extension install word-count --from ./word-count
relate run word-count -d ./store
# word-count: imported 1284 words from 37 files, 1284 WORDS edges
#   MATCH (w:Word) RETURN w.text AS word ORDER BY w.count DESC LIMIT 10
relate query --database ./store "MATCH (w:Word) RETURN w.text AS word ORDER BY w.count DESC LIMIT 10"
relate extension remove word-count
```

The stages behind each `check` line are
[the check verb](spec.md#the-check-verb); the shipped reference
children are in [`examples/`](examples/) (bash, Python, TypeScript) and the
workspace's `extensions/` group (Rust).

## Publishing

The signed first-party channel and its tarball contract are the release
pipeline's ([spec.md](spec.md), "The channel and its publisher"). A package
you authored outside this repository ships through `--url` (https only) or
as a directory the user installs with `--from`.
