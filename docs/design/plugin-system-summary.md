# Plugin system — executive summary (2026-09-25)

**Question**: Lua, Python, or something else as the extension mechanism for
Relate — with safety, speed, contributor-friendliness, and an agent code
sandbox in mind.

**Recommendation: neither Lua nor Python. WASM guests on wasmtime, with a
deny-by-default, capability-gated host ABI.**

## Why

- **Safety**: WASM linear memory + deny-by-default host functions is the only
  option strong enough for agent-generated/untrusted code. Lua's sandbox has a
  CVE history and only Luau mode is sandboxed; Python-in-process can't be
  sandboxed at all; native dylibs have no boundary (SQLite's model — the one
  everyone now warns about).
- **Speed**: wasmtime gives ~5 ms warm / ~16 ms cold per call with AOT caching
  (measured by the denyx project) — irrelevant next to IO, which is what
  loader plugins actually wait on.
- **Contributions**: any language compiles to WASM (Rust, Go, JS/TS, Python
  via py2wasm later) — one host ABI, many guest languages. Extism already
  proves the shape.
- **Fit**: one binary, no external runtimes; the same runtime serves the
  future browser console.

## Prior art read-across

| System | Mechanism | Lesson for us |
|---|---|---|
| SQLite | C dylibs + virtual tables | vtab = the row-source model; dylib = the unsafe model to avoid |
| DuckDB | signed C++ extensions, community repo, WASM target | distribution/registry pattern; signatures ≠ sandboxing |
| ClickHouse | executable UDFs (out-of-process), pooled, network-gated to allowed hosts | the exact shape for SaaS loaders — we get it in-process via WASM |
| Neo4j | in-JVM procedures | the language-extension trap we decline |
| PostgreSQL | C ext + trusted/untrusted PLs | the "trusted vs untrusted language" split |
| denyx / sandboxd | wasmtime sandbox: fuel + epoch + memory fences, deny-by-default ABI | reference design, measured costs |

## Scope decision (answers the "how far?" question)

Two extension roles, one runtime: **row-source loaders/transformers**
(`CALL plugin.rows('github', $args) YIELD …`) and **agent code sandbox**
(`CALL plugin.run('analysis', $rows)` with fuel/time/memory limits).
**Not** in v1: custom query-language features, user-defined Cypher procedures/
functions, engine internals. Those stay in-repo Rust where the TCK can gate
them. Escalation is an ADR, not an accident.

## Next step

Write the ADR ("extension mechanism = capability-gated WASM guests") with
`plugin-system-analysis.md` as Context; then prototype `relate-ext` with
`CALL plugin.rows` and two demo loaders (remote CSV, GitHub API).
