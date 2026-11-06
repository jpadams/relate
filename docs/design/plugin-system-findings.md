# Plugin system research — findings (2026-09-25)

Raw facts gathered for the Relate extension-mechanism decision. Distilled
in `plugin-system-analysis.md`, executive version in `plugin-system-summary.md`.

## Prior art in databases

**SQLite**
- Loadable C extensions (`.so`/`.dll`, `sqlite3_load_extension`) — powerful but
  fully unsafe: C ABI in-process, no sandbox, frequently disabled in hosted
  contexts. This is the model that gave SQLite its ecosystem (FTS5, spatialite)
  but also the reason everyone else rejects it today.
- Virtual tables (vtab API) are the *data-source* extension point — a module
  presents remote/computed data as a queryable table. Directly analogous to
  "loaders that invoke Cypher / read remote sources and emit rows".

**PostgreSQL**
- C extensions (in-process, unsafe, superuser-gated) + PL/* (plpython3u marked
  UNtrusted, plperl/pltcl have trusted variants, plv8). pgrx = Rust framework
  for extensions (safe-ish Rust → C ABI, still in-process).
- Lesson: "trusted language" (runtime cannot do IO/syscalls) vs "untrusted"
  (full access) is a useful two-tier framing for us too.

**DuckDB** (verified 2026-09, duckdb.org/community_extensions)
- Community Extensions repo: `INSTALL h3 FROM community; LOAD h3;` — developer
  PRs a descriptor file; DuckDB's CI builds + signs binaries for
  Linux/macOS/Windows/**WASM**; signature + platform/version checked on load.
- Explicit caveat: "DuckDB Labs and the DuckDB Foundation do not vet the code
  within community extensions and cannot guarantee they are safe" — trust model
  is signature+opt-out, not sandboxing. In-process C++ ABI.
- Also has a newer experimental C Extension API (duckdb/extension-template-c).

**ClickHouse** (verified 2026-09, clickhouse.com/docs)
- Executable UDFs: out-of-process programs (Python, Bash, anything), data
  streamed via STDIN/STDOUT in blocks; `executable_pool` keeps long-lived
  process pools; timeouts, SIGTERM lifecycle. Per-call overhead vs in-process
  is acknowledged; recommended for "heavier logic or a different runtime".
- ClickHouse Cloud beta adds managed sandboxed pools + **network-access UDFs
  gated to allowed hosts** — the exact shape of "loader plugins that call SaaS
  APIs".
- `executable(...)` table function = row-producing external scripts.
- Lesson: out-of-process is the safe way to run arbitrary user code, at the
  cost of a second runtime on the host (Python env) and process overhead.

**Neo4j**
- Custom procedures/functions = in-process JVM (Java), server-side, no sandbox;
  APOC is the giant showcase. Not a fit for an embedded single-binary engine;
  this is the thing we explicitly don't want to replicate in-process.

**Redis**
- Embedded Lua (5.1, now also Functions API). Sandboxed *scripting*, but the
  sandbox has a long CVE history (library escapes). Lua's strength is
  request-level scripting, not plugins.

**Weaviate / others**
- Weaviate modules: out-of-process gRPC/HTTP services — again the
  out-of-process pattern for heavy/remote logic.

## Runtime candidates (Rust ecosystem, current as of 2026-09)

From khvzak/script-bench-rs (M5 Max, rustc 1.96) and crate docs — the current
benchmarked inventory: **boa** 0.21, **koto** 0.16, **mlua** 0.12-rc (Lua 5.4/
LuaJIT/Roblox Luau), **rhai** 1.25, **roto** 0.11, **rquickjs** 0.12,
**wasmi** 1.0, **wasmtime** 45.

- **mlua**: Lua/Luau bindings; `Lua::sandbox` exists for Luau only; "module
  mode" explicitly unsafe (external Lua runtime, debug lib). Safe path = Luau
  sandbox. Lua ecosystem familiarity is high (Redis, Neovim, nginx) but its
  plugin "module" story is C; data transforms only.
- **rquickjs** (QuickJS-NG): 210 KiB, <300 µs runtime lifecycle, ES2020,
  Rust↔JS promise bridging, `embed` macro for bytecode; **no system/web APIs**
  by design (host must supply IO); also compiles to wasm32-wasip2 (JS-in-WASM
  possible). Windows/Apple-aarch64 support marked experimental — a risk for a
  cross-platform single binary.
- **Starlark** (starlark-rust): hermetic by design (no IO, deterministic),
  Python-ish syntax — but pure-computation runaway scripts have **no cheap
  preemption** in-process (denyx's finding below).
- **wasmi**: pure-Rust WASM interpreter — slower than wasmtime but smaller,
  no JIT, works on more targets (incl. no-JIT platforms).
- **wasmtime**: Cranelift JIT, mature limits: `consume_fuel` (deterministic CPU
  budget), `epoch_interruption` (wall-clock fence), `ResourceLimiter` (memory),
  bounds-checked linear memory, AOT `.cwasm` cache.

## Sandbox case studies (directly relevant)

**denyx** (github.com/Spin42/denyx, docs/wasm-sandbox.md) — runs a
starlark-rust interpreter compiled to wasm32-wasip1 inside wasmtime:
- Fuel handles pure-CPU runaways (in-process starlark couldn't); epoch watchdog
  handles wall time; host-side policy gate stays in Rust.
- Measured: ~5 ms warm per-call cost with cached Engine+Module, ~16.5 ms cold
  with AOT deserialize; interpreter aborts surface as typed traps, not host
  crashes (empirically pentested).
- Notes gaps honestly: fuel doesn't bound memory growth; fallback JIT ~480 ms.

**sandboxd** (github.com/sarmakska/sandboxd) — wasmtime wrapper for untrusted
code: fuel + epoch watchdog + ResourceLimiter as three independent fences,
deny-by-default host ABI (no WASI at all), typed `SandboxError` per failure
mode. Design line worth stealing: "a guest cannot name an address it was not
given, cannot call a function it was not handed".

**Extism** (extism.org) — universal WASM plugin system: host application +
`.wasm` guests via language PDKs (Rust, Go, JS/TS, Python, C#, …), host
functions expose capabilities (their example is a KV store), guest languages
share one ABI. Practically a reference implementation of the shape we'd want;
usable as-is or as the API sketch.

**raisfast** (docs.rs/raisfast plugins.rs) — a real Rust service shipping four
runtime backends behind feature flags (`plugin-wasm/-js/-lua/-rhai`), with
host APIs `httpGet`, `dbQuery`, `getData/setData` — confirms the multi-runtime
shape is common, but doubles the audit surface.

## Two-seats problem

Everything we want splits cleanly into two extension *roles*:

1. **Loader/transformer plugins** ("row sources"): read remote/structured data,
  produce rows (`MATCH`-able), may call Cypher host-side. = SQLite vtab,
  ClickHouse executable table function. Data-heavy, capability = network/file
  read allowlist.
2. **Agent code sandbox**: user/agent-supplied code for analytics, compute over
  query results, agent-memory/context logic. = ClickHouse executable UDFs,
  denyx/sandboxd threat model. Untrusted by definition, needs fuel+time+memory
  fences, deny-by-default capabilities.

Both roles share one runtime if the host ABI is capability-gated.
