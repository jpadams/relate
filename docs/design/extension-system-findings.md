# Extension system research — findings (2026-11-01)

Raw research for "what would a proper extension system look like — one that can
extend storage, engine, Cypher, surface and CLI" (DuckDB-style), as opposed to
the shipped child-process command extensions (ADR-0429/0431/0434/0442/0447).

Companion to the earlier loader-plugin study (`plugin-system-{findings,analysis,summary}.md`,
2026-09-25), which recommended capability-gated WASM guests for *loaders* and
explicitly deferred engine internals to in-repo Rust.

---

## 1. SQLite and PostgreSQL

*Research note, 2026-11-01. PostgreSQL references are to the 18 docs ("current");
SQLite references to current sqlite.org pages.*

### SQLite

**Loadable extensions: mechanism and calling convention.**
`sqlite3_load_extension(db, zFile, zProc, pzErrMsg)` dlopens a shared library and
calls one entry point: `int init(sqlite3 *db, char **pzErrMsg, const sqlite3_api_routines *pApi)`.
With `zProc == NULL`, SQLite tries `sqlite3_extension_init`, then `sqlite3_X_init`
(X = lower-cased filename, directory and extension stripped, leading `lib` dropped).
Loading is per-connection; `SQLITE_OK_LOAD_PERMANENTLY` keeps the library mapped
after close (required for VFS extensions), and `sqlite3_auto_extension()` registers
an entry point that runs on every new connection.

Build contract: include `sqlite3ext.h` (not `sqlite3.h`), `SQLITE_EXTENSION_INIT1`
on its own line after the include, `SQLITE_EXTENSION_INIT2(pApi)` first in the entry
point, `SQLITE_EXTENSION_INIT3` for the `extern` in further translation units. With
`SQLITE_CORE` defined (or `SQLITE_OMIT_LOAD_EXTENSION`) all three are no-ops and the
extension links real `sqlite3_*` symbols statically. Otherwise every `sqlite3_*` call
is a macro for `sqlite3_api->...` — **a function-pointer table handed in by the core.
That indirection is the ABI story**: an extension never links libsqlite3, so the core
can grow by appending table entries.

| Extendable | API |
| --- | --- |
| Scalar/aggregate functions | `sqlite3_create_function_v2()` |
| Window functions | `sqlite3_create_window_function()` (3.25+) |
| Collations | `sqlite3_create_collation_v2()` |
| Virtual tables | `sqlite3_create_module_v2()`, `sqlite3_module` + `xBestIndex` |
| VFS (OS layer) | `sqlite3_vfs_register()` |
| Busy handler | `sqlite3_busy_handler()` |
| Auto-extension | `sqlite3_auto_extension()` |
| App-level observation | authorizer, progress, update, trace, commit hooks |

Not extendable: parser, planner, b-tree/pager/WAL internals. The registration surface
is the whole surface; the only planner participation an extension gets is `xBestIndex`
for its own virtual table. Below that means building modified SQLite from canonical source.

**Why the model works.** "Stable interfaces will be maintained indefinitely in a
backwards compatible way"; an app using only stable interfaces "should always be able
to relink against a newer version of SQLite". Version numbering is semantic since 3.9.0,
`X` stays 3, and the project plans to support the file format, SQL syntax and C interface
*through at least 2050*. Versioned structs (`sqlite3_vfs.iVersion`, `sqlite3_module.iVersion`)
allow append-only method tables.

Compile-time vs load-time: the amalgamation bundles FTS5, RTREE, SESSION etc. behind
`#ifdef`, enabled with `-DSQLITE_ENABLE_*`. `SQLITE_OMIT_*` is explicitly *unsupported*
and usually only works when built from canonical sources — a vendor/OEM tool, not a
deployment mechanism. Runtime extension is the portable customization path.

Security: loading is off by default; enable with
`sqlite3_db_config(db, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 1, NULL)` (not
`sqlite3_enable_load_extension()`), so the SQL `load_extension()` stays disabled and
SQL injection cannot load a `.so`.

### PostgreSQL

**Model.** A C extension is a shared library in `$libdir` plus
`SHAREDIR/extension/<name>.control` and versioned SQL scripts `<name>--<ver>.sql`;
upgrades are `<name>--<old>--<new>.sql`. `ALTER EXTENSION UPDATE` chains scripts.
Control fields: `default_version`, `module_pathname`, `requires`, `relocatable` /
`schema` / `@extschema@`, `superuser` (default true), `trusted`. `CREATE EXTENSION` is
per-database (install into `template1` for inheritance), records object membership in
`pg_extension`. PG 18 adds `extension_control_path`. PGXS builds and installs the `.so`
plus control/script files; deliberately not a general build system.

| Capability | How installed |
| --- | --- |
| Types, operators, opclasses, casts, aggregates, C/SQL/PL functions | `CREATE EXTENSION` |
| Index AM (`CREATE ACCESS METHOD … TYPE INDEX`) | `CREATE EXTENSION` |
| Table AM (`… TYPE TABLE`) | `CREATE EXTENSION` |
| FDW + foreign tables | `CREATE EXTENSION`, then `CREATE FOREIGN DATA WRAPPER/SERVER/TABLE` |
| Logical decoding output plugin | loaded by walsender, no preload |
| Custom GUCs (`DefineCustom*Variable` in `_PG_init`) | `CREATE EXTENSION` / on load |
| Planner/executor hooks (`planner_hook`, `set_rel_pathlist_hook`, `create_upper_paths_hook`, `ExecutorStart/Run/Finish/End`, `post_parse_analyze_hook`, `ProcessUtility_hook`) | plain globals; global effect ⇒ `shared_preload_libraries` |
| Custom scan provider (docs: "experimental") | via hooks, same rule |
| Background workers, shared memory (`shmem_request_hook`) | `shared_preload_libraries` |
| Custom WAL resource manager (`RegisterCustomRmgr`) | `shared_preload_libraries` |

The dividing line is the postmaster: anything that must exist before backends fork
(workers, shmem, cluster-wide hooks) needs a restart. Everything per-backend installs
without one but is then session-scoped unless preloaded. Custom rmgr IDs are 128–255;
docs warn the library must stay preloaded while custom WAL may exist, or the server may
not start — and the wiki warns custom rmgrs can break disaggregated-storage forks
(Aurora, Neon, Alloy).

**ABI: no stability across majors, by design.** Postgres' own guidance (Peter
Eisentraut, committed to the C-language docs 2024-07-31): "There is *no* promise of API
compatibility between PostgreSQL major versions" and servers of different majors "have
intentionally incompatible ABIs … must be re-compiled for each major release."
`PG_MODULE_MAGIC` embeds compile-time ABI constants (version, `FUNC_MAX_ARGS`,
`INDEX_MAX_KEYS`, `NAMEDATALEN`, `FLOAT8PASSBYVAL`) so a mismatched build is rejected
at load rather than crashing; PG 18 adds `PG_MODULE_MAGIC_EXT` with name/version
metadata. Minor releases try to avoid ABI breaks (pad-space reuse, append-only struct
growth) but do not guarantee it. Practical cost: one source tree × N majors,
`#if PG_VERSION_NUM >= …` in hot spots, a rebuild per major — PG 18's `TupleDescData`
rework forced pgrx to redo its bindings.

Why the community accepts it: extensions get the *same* APIs core uses (Node trees,
tuple slots, buffer manager) — that is what makes PostGIS/pgvector-class extensions
possible. Freezing that ABI would freeze the internals Postgres refactors to get
faster. Core manages the well-delimited interfaces (fmgr, SPI, hooks) for stability;
the rest of the global symbol space is usable but not designed for it.

**Distribution and trust.** `contrib/` ships with the server; third-party via PGXN,
apt/yum, source, or cloud allowlists. A C extension is arbitrary code inside the backend
with the `postgres` OS user's privileges. `CREATE EXTENSION` is superuser-only unless
the control file sets `trusted = true`, which lets any user with `CREATE` install it —
with the script running as bootstrap superuser, so the docs require auditing it as
hostile input. Trusted *languages* (PL/Rust via `postgrestd` + a custom rustc driver)
are the other route. Code in `shared_preload_libraries` runs in the postmaster: a crash
takes the cluster, not one session.

**Rust: pgrx.** A framework plus `cargo-pgrx`: generates `pg_sys` bindings per major,
derives SQL from Rust (`#[pg_extern]`), supplies `pg_module_magic!()`, converts panics
into `ERROR`s via `#[pg_guard]`, routes allocation through palloc. It does not abstract
the ABI away — it re-exposes it with safety rails, one Cargo feature per major
(`pg13`…`pg18`). v0.19.3 (2026-09-27) supports PostgreSQL 13–18 plus 19beta4, with
per-version code paths where core changed. PL/Rust builds on pgrx to make Rust a
*trusted* procedural language.

**Sources.** sqlite.org/loadext.html · sqlite.org/c3ref/load_extension.html ·
sqlite.org/c3ref/auto_extension.html · github.com/sqlite/sqlite/src/sqlite3ext.h ·
sqlite.org/c3ref/db_config.html · sqlite.org/capi3ref.html · sqlite.org/versionnumbers.html ·
sqlite.org/onefile.html · sqlite.org/compile.html · sqlite.org/vtab.html · sqlite.org/vfs.html ·
postgresql.org/docs/current/{extend-extensions,sql-createextension,sql-alterextension,catalog-pg-extension,extend-pgxs,xfunc-c,custom-rmgr,wal-for-extensions,custom-scan,bgworker,logicaldecoding}.html ·
wiki.postgresql.org/wiki/CustomWALResourceManagers ·
postgresql.org/message-id/16518d94-88d2-4b58-bdd4-c91ab020bf4e%40eisentraut.org ·
justatheory.com/2024/06/abi-api-guidance/ · github.com/pgcentralfoundation/pgrx ·
plrust.io · github.com/pgcentralfoundation/plrust

---

## 2. Loading third-party extensions in Rust — state of practice (2026-11-01)

Rust has no stable ABI, so every plugin system is a choice about *where* to put the
instability: a hand-built C boundary, a Rust-shaped stable-ABI crate, a bytecode
sandbox, or a process boundary.

### cdylib + C ABI (SQLite / DuckDB model)

**What can cross:** `#[repr(C)]` structs, integers, floats, raw pointers,
`extern "C" fn`. **What cannot:** `String`/`Vec`/`Result`/`Option<T>` (layout and niche
optimisation not guaranteed), generics (monomorphised per crate), `dyn Trait` (vtable
layout compiler-private), `async fn` (opaque future layout).

**The "host services" pattern.** Pass a struct of function pointers, never a Rust type:
`plugin_init(*const HostApi) -> *const PluginApi`. Both sides call only through tables,
so capabilities can be added without a new entry point. DuckDB is the canonical
implementation: `duckdb_ext_api_v1` is an append-only function-pointer struct with three
ABI classes — `CPP` (exact DuckDB version), `C_STRUCT` (minimum version, stable since
C API v1.2.0), `C_STRUCT_UNSTABLE` (exact version) (duckdb#14992, 2024-11-26; versioning
docs). The header's own warning: *"this API is not yet stable"* (duckdb_extension.h).

**Panics.** Unwinding past a non-unwinding frame is UB (Rust Reference); a panic escaping
`extern "C"` aborts under `panic=unwind` (RFC 2945). `catch_unwind` catches only
unwinding panics — never abort — and catching a *foreign* unwind is explicitly
unspecified. Wrap every exported entry point in `catch_unwind` + `AssertUnwindSafe`,
convert to an error code, document that plugin state may be poisoned afterwards.

**Memory.** Never assume a shared allocator. Rule: *the side that allocated frees.*
Return owned buffers as `(ptr, len, cap)` plus an exported `free`, or put an allocator in
the services table. Never `Vec::from_raw_parts` on a foreign pointer; `capacity` is part
of the contract.

**Symbols and unloading.** `#[no_mangle] pub extern "C"` is the only exported kind from a
`cdylib`. Symbol collisions between `.so`s are silent at link time — version the entry
symbol (`myapp_plugin_init_v1`). `dlclose` while vtables or callbacks are live is a
use-after-free; SQLite keeps extensions loaded for the connection lifetime, `abi_stable`
refuses to support unloading at all.

### Stable-Rust-ABI crates

| Crate | Latest | Mechanism | Honest limit |
| --- | --- | --- | --- |
| **abi_stable** | 0.11.3, **2023-10-12**; 4.3M downloads, 91 dependents | `#[derive(StableAbi)]`, `RBox`/`RStr`/`ROption`, `#[sabi_trait]`, prefix-type vtables, recursive layout check at load | Unmaintained since 2023; "assumes dynamic libraries come from a benign source"; no unloading; heavy `Drop` overhead |
| **stabby** | 72.1.16, **2026-07-20**; 8.6M downloads, 18 dependents | `#[stabby::stabby]`, niche-optimised `#[repr(stabby)]` enums, ABI-stable vtables/futures, type reports + canaries | SemVer-Prime versioning (72.x); trait-object vtables in a leaked global set since Rust 1.78; still an opt-in per-type universe |
| **crABI** | RFC #3470, open since 2023-08-11, touched 2026-10-02 | `extern "crabi"` / `repr(crabi)` lowering to the C ABI | Open, unmerged RFC with an experimental feature gate — **not usable** |

They convert "silent UB on mismatch" into "loud load error", which is the real win. Where
the host does *not* use them the requirement is brutal: Zenoh's plugin docs demand *"the
exact same Rust version"* as `zenohd`, "otherwise incompatibilities in memory mapping …
can lead to a SIGSEGV". The Tremor experiment measured ~30% slowdown for an
`abi_stable`-based plugin boundary — workable but heavy.

### WebAssembly as the plugin ABI

Wasmtime is the reference runtime — 50.0.1 (2026-10-29), WASI 0.3.0 by default since
46.0.0 (2026-06-22). Standards gap: WASI 0.3 shipped 2026-06-11 and is in production,
but the Component Model still sits at **Phase 1** in the CG proposal registry; the new
W3C WG charter (2026-08-20) makes it a deliverable only *after* Phase 4. Rust's
`wasm32-wasip3` target is nightly-only until Rust 1.100 (2026-11-12); `wasm32-wasip2` is
stable.

**Guarantees:** linear memory + deny-by-default imports means no ambient
filesystem/network; a trap kills the instance, not the host; the interface is language-
and compiler-independent. **Costs:** every crossing copies through linear memory; the
only shared thing is bytes, so you pick a serialisation (WIT/canonical ABI, or
JSON/protobuf à la Extism).

**Measured cost** (fidius benchmark): a tiny call is ~34 ns via cdylib versus ~24 µs via
a wasmtime component — WASM lands in the same band as localhost HTTP (~17 µs) and
Unix-socket IPC (~6 µs). Wasmtime core-call overhead itself is ~4 ns; the 24 µs is
store/instance/marshalling, so pre-instantiation and pooling matter (Spin: 25 µs
pre-instanced → 11.8 µs pooled → 6.7 µs reused).

**Adopters:** Zed (component model + WIT, `wasm32-wasip2`, epoch deadlines, capability
granter, 2024-10-21); Zellij (`wasmi`, `wasm32-wasip1`, protobuf contracts, thread-pinned
instances, `StoreLimits`); Envoy (Proxy-Wasm ABI v0.2.1 over V8/WAMR/Wasmtime, per-plugin
capability restrictions); SingleStore (`wasmtime`, 16 MB default sandbox, Basic vs
Canonical/WIT ABI, no syscalls or threads); ClickHouse (experimental in-process WASM
UDFs); BigQuery (JS UDFs hosting WASM, e.g. DataSketches C++ → wasm, 2025-05-22).

### Out-of-process plugins

A child process over stdio or a socket gives crash isolation, language freedom and zero
ABI coupling — you version a *message schema*, not a compiler. Cost: ~6–20 µs per round
trip, no shared memory, lifecycle management (pools, timeouts, restart).

- **Nushell**: `nu_plugin_*` executables, JSON or MessagePack, `--stdio` or local socket,
  `Hello` handshake carrying protocol version + feature flags, garbage collection.
- **ClickHouse**: executable UDFs stream blocks over stdin/stdout; `executable_pool` of
  long-lived processes is the recommended mode, with read/write/termination timeouts.
- **Neo4j**: the counter-example — extensions are JARs in `plugins/`, in-JVM, with the
  docs calling unmanaged extensions "a sharp tool". Neo4j puts a process/protocol
  boundary at Bolt, not at the extension boundary.

### What real hosts actually chose

| Host | Mechanism | Why |
| --- | --- | --- |
| rust-analyzer | **Hybrid**: proc-macro dylib loaded by a *separate* `proc-macro-srv` process, version-checked against `.rustc` metadata, multi-ABI modules per rustc version | The ABI is unavoidable; the process boundary contains the crash (#12803) |
| Zed | wasm component + WIT | Untrusted extensions, cross-language, crash containment |
| Zellij | wasm (`wasmi` interpreter) | Sandboxing, portable distribution |
| Envoy | wasm (Proxy-Wasm) | No stable C++ ABI; safe filter code from many languages |
| Nushell | out-of-process stdio | Language freedom, plugin crashes don't kill the shell |
| Helix | **Steel (Scheme) interpreter**, not wasm | Fast iteration, no ABI at all; a wasm+Lua PR was closed in 2025 as not viable (#13953) |
| Deno | `op`s compiled *into* the runtime (`deno_core::extension!`) | Not third-party native loading; permission checks at the op boundary |
| Zenoh | Rust `dylib`, exact-toolchain requirement | Performance; documented SIGSEGV risk — the cautionary tale |

**Failure modes to design around:** ABI drift (DuckDB `CPP`, proc-macro `.rustc`
mismatch); double free / allocator mismatch; panic across FFI (abort or UB); version skew
(Nushell's `Hello`, WASI tool pinning — Wasmtime 43–45 vs 46 emit "wrong type" at
instantiation); symbol collisions; unload-while-live; thread-unsafe `dlerror` on some
platforms. The Pumpkin Minecraft server migrated its whole plugin API from native to
WASM/WIT in 2026 for exactly this list (2026-05-11).

### Decision table

| Mechanism | Language reach | Safety | Per-call cost | Version coupling | Distribution |
| --- | --- | --- | --- | --- | --- |
| cdylib + C ABI | C ABI only | None (trusted code) | ~ns | Your own table version | Per-OS/arch binary |
| abi_stable / stabby | Rust only | Layout-checked, no sandbox | ~ns (+Drop overhead) | Crate major + rustc-ish | Per-OS/arch binary |
| Wasm core/WASI | Any wasm target | Strong sandbox | ~µs–tens of µs | WIT/WASI version, not rustc | One `.wasm` |
| Wasm component + WIT | Any component toolchain | Strong sandbox, typed | ~24 µs (pooled lower) | WIT world version | One `.wasm` |
| Out-of-process (stdio/IPC) | Any language | OS process isolation | ~6–20 µs | Message schema | Any executable |

**Rule of thumb:** trusted, performance-critical, Rust-only → `abi_stable`/`stabby` with a
load-time check and no unloading. Untrusted or polyglot → wasm with WIT (accept the µs).
Anything long-running, crash-prone or requiring a foreign runtime → out-of-process. Never
raw `dylib` + Rust types.

**Sources:** Rust Reference panic/UB pages (current) · RFC 2945 · docs.rs/abi_stable +
crates.io (0.11.3, 2023-10-12) · github.com/ZettaScaleLabs/stabby + crates.io (72.1.16,
2026-07-20) · rust-lang/rfcs#3470 (open, 2026-10-02) · duckdb/duckdb#14992 (2024-11-26) ·
sqlite.org/loadext.html · wasmtime releases (50.0.1, 2026-10-29) · WebAssembly/component-model
milestones · zed.dev/blog/zed-decoded-extensions (2024-10-21) · proxy-wasm/spec ·
extism.org/docs/concepts/host-functions · nushell.sh/contributor-book/plugin_protocol_reference.html ·
docs.singlestore.com/cloud/reference/code-engine-powered-by-wasm · clickhouse.com/docs/reference/functions ·
rust-lang/rust-analyzer#12803 · colliery-io.github.io/fidius/explanation/performance

---

## 3. Other database extension systems — design survey (2026-11-01)

Scope: what can be extended (storage, engine/planner, query language, protocol
surface, CLI/ops), how it loads, how it versions/trusts code, and the maintenance cost.

### Neo4j

JVM-only. `@Procedure` / `@UserFunction` with `READ|WRITE|SCHEMA|DBMS` modes; `@Context`
injects `Log`, `TerminationGuard`, `GraphDatabaseService`, `Transaction`. Unmanaged
JAX-RS server extensions still exist in 2026.10, mounted with
`server.unmanaged_extension_classes`. There is no non-JVM extension path.

Loading: JARs in `$NEO4J_HOME/plugins`, loaded at startup. APOC Core ships in `labs/`
and is officially supported since Neo4j 5; APOC Extended is community-only. Trust:
`dbms.security.procedures.allowlist` defaults to `*` (all plugin JARs load),
`unrestricted` defaults empty and gates internal APIs; Enterprise adds `EXECUTE PROCEDURE`
and boosted-procedure privileges. No general signing, sandbox or ABI negotiation:
compatibility means matching the calendar version (APOC 2026.10.x ↔ Neo4j 2026.10.x).
Cost: in-process full trust, and a mismatched JAR can block startup.

### ClickHouse

No general dynamic plugin API, deliberately. A maintainer stated in 2020 that
shared-library plugins were rejected for maintainability, with ODBC and library
dictionaries moved to bridge processes (issue #1053, comment 2020-07-14). Extension
points are declarative or compiled-in: dictionaries (sources/layouts via DDL/config),
table functions/engines, SQL UDFs, executable UDFs over stdin/stdout, formats, settings,
and `contrib/` submodules linked into the binary.

The 2026 change: experimental WebAssembly UDFs. Modules go into
`system.webassembly_modules`, functions are `CREATE FUNCTION ... LANGUAGE WASM`, with
`ROW_DIRECT`, `BUFFERED_V1` or `ASSEMBLYSCRIPT` ABIs and wasmtime. The WASM-plugin issue
was closed 2026-02-26. Cost: new engines/functions still mean patching C++ and rebuilding.

### MySQL/MariaDB

Plugin API since 5.1: storage engines, UDFs, audit, authentication, replication, keyring,
clone, full-text parsers. `INSTALL PLUGIN ... SONAME`; the library exports
`_mysql_plugin_interface_version_` and `_mysql_plugin_declarations_`, with a per-type
interface version as the first descriptor field. MySQL 8 components add named service
APIs in a registry: components consume and provide services, install with
`INSTALL COMPONENT`, designed to replace plugins while both coexist. But headers are
internal and there is no version negotiation. A 2026-05-08 bug documents a plugin built
on 8.4.8 failing on 8.4.9 and across MySQL-compatible distributions; VillageSQL proposes
a versioned ABI plus `INSTALL EXTENSION` (bug 120427). Cost: the highest
portability/maintenance burden of the group.

### Redis/Valkey

Redis Modules: one C header (`redismodule.h`), `RedisModule_OnLoad`, API version `"1"`.
Modules register commands, new data types (`RM_CreateDataType`, `encver` 0–1023, RDB/AOF
callbacks, `REDISMODULE_TYPE_METHOD_VERSION` 5), ACL categories (7.4), command metadata
(7.0) and configs (8.2). Versioning is **capability probing**: absent functions are NULL,
so one binary spans releases. Trust is in-process native code; ACLs gate commands, not
module code. Valkey keeps ABI compatibility with the Redis OSS 7.2 module API under both
`ValkeyModule_` and `RedisModule_` names.

### Wasm-based extensibility

- **SingleStore**: wasmtime, one in-process sandbox per function, 16 MB default, Basic
  and Canonical ABI (WIT), UDF/UDAF/TVF, included in backups; no syscalls, files,
  sockets or threads (2025-06-13).
- **BigQuery**: C++ DataSketches compiled to WASM and loaded inside JS UDAFs
  (2025-05-22). Library delivery, not a plugin ABI.
- **Databricks**: Python UDFs with custom dependencies, batches and service credentials
  (2025-06-23); its 2026 WASM story is PGlite, not database plugins.
- **Databend**: Python/JS/WASM UDFs plus Arrow Flight external UDF servers (2025-06-26).
  **RisingWave**: Rust→WASM embedded, Arrow Flight external functions. **Firebolt**:
  Python-only UDFs, private preview, explicitly no WASM.
- **Cloudflare**: Workers runs WASM; D1/Durable Objects are SQLite-backed; extension
  happens at the application layer, not via a database plugin API.
- **Extism**: off-the-shelf host/PDK system using JSON/bytes and host functions, not the
  component model (maintainers' stance, issue #666, 2024-07-25). Helm 4 adopted it for
  CLI plugins (HIP-0026, 2025).
- **Component model**: SingleStore's Canonical ABI and Uni's WASM Component Model loader
  (alongside Extism/Rhai/PyO3) show typed WIT contracts with declared∩granted
  capabilities. No major database ships component-model storage plugins.

### Adjacent designs worth stealing

- **VS Code**: manifest `contributes` + `activationEvents`, lazy activation,
  out-of-process extension host, `engines.vscode` minimum, extensions exporting APIs to
  each other.
- **PostgreSQL**: broadest surface — FDW, table access methods, hooks, types, operators,
  PLs. 441 extensions, but 16.8% are pairwise incompatible and hook chaining is
  unverifiable (PVLDB 18, 2025, p1962-kim).
- **OTel Collector**: component factories plus per-signal stability levels
  (development/alpha/beta/stable), and custom distributions assembled from chosen
  components; stable requires 80% coverage, lifecycle and benchmark tests.
- **Envoy**: `typed_config` type URL is a versioned schema; filters hot-reconfigure;
  `fail_open` is per-plugin and defaults closed (Proxy-Wasm ABI 0.2.1).
- **Grafana**: backend plugin is a subprocess over gRPC via HashiCorp go-plugin, so
  panics cannot kill the server; signed `MANIFEST.txt` required by default; protocol
  major versions supported side by side.
- **Kafka Connect**: one classloader per plugin directory under `plugin.path`,
  child-first; KIP-891 adds soft/hard version ranges and multiple installed versions.

### Comparison

| System | Extends | Loading | Versioning / trust | Main cost |
| --- | --- | --- | --- | --- |
| Neo4j | Cypher procs/functions, auth, HTTP surface | JVM JARs, in-process | calendar match; allowlist/unrestricted; no sandbox | full trust; version skew |
| ClickHouse | dictionaries, UDFs, formats; engines via C++ | config/DDL, subprocess, wasm | experimental wasm; no plugin ABI | fork/upstream for depth |
| MySQL | storage, UDF, audit, auth, replication | `.so` plugin or component | internal headers, no negotiation | ABI fragility |
| Redis/Valkey | commands, types, ACLs, configs | C shared library | API v1 + NULL probing | native crash risk |
| DuckDB | types, functions, scans, storage | C++ or C ABI `.duckdb_extension` | CPP exact; C_STRUCT min-version; signed repo | rebuild per release |
| SingleStore | scalar/aggregate/table functions | wasmtime sandbox | WIT/Canonical or Basic ABI | no host I/O |
| Extism | arbitrary host functions | wasm, in-process | host-fn ABI; no component model | ABI is host-defined |
| Grafana | data sources, apps, endpoints | subprocess/gRPC | signed manifest, protocol majors | packaging/latency |
| OTel | receivers/processors/exporters/extensions | compile-time factories | per-signal stability | rebuild distribution |

### Ranked shortlist for an embedded Rust graph database

1. **Compile-time Rust trait registry behind feature bundles (trusted core).** Best for
   storage, planner/executor and Cypher dialect, where hooks need zero-copy typed access
   and must preserve invariants. Relate already has `full`/`agent`/`minimal` bundles
   and a C ABI, so extend that: one `Extension` trait family, a registry populated at
   build time, and an OTel-style stability label per capability. Cost: users rebuild the
   binary; no third-party drop-in.
2. **DuckDB-style versioned C ABI for third-party in-process plugins.** Copy the
   append-only function-pointer struct with a declared minimum host version (`C_STRUCT`),
   a separate unstable section, per-platform/per-host-version builds, and signed
   artifacts. Redis adds the useful capability-probing idiom. Use for storage backends and
   scalar functions written in any language; accept crash/UB risk and per-release rebuilds.
3. **Grafana-style out-of-process sidecar over gRPC/JSON-RPC.** Best for network surfaces
   (Bolt/HTTP/MCP adapters) and CLI subcommands: crash isolation, language freedom, clean
   version negotiation, no ABI coupling. Cost: process lifecycle, latency, packaging.
4. **WASM (component model preferred, Extism fallback) for untrusted UDFs and expression
   hooks.** Sandboxed and portable, with WIT-typed contracts where available; keep it out
   of storage/engine paths because of data-copy overhead and the immature component-model
   toolchain.
5. **Cross-cutting governance: VS Code manifest + OTel stability + Envoy fail-closed.**
   A declarative manifest with contribution points and activation events, explicit
   stability tiers, typed configs and fail-closed loading gives a coherent UX across all
   four tiers without weakening the trust boundary.

**Sources:** neo4j.com/docs/java-reference/current/extending-neo4j · neo4j.com/docs/operations-manual/current/{configuration/plugins,security/securing-extensions} ·
github.com/ClickHouse/ClickHouse/issues/1053 (2020-07-14) · clickhouse.com/docs/reference/functions/regular-functions/wasm_udf ·
dev.mysql.com/doc/extending-mysql/8.0/en/server-plugin-descriptors.html · dev.mysql.com/doc/dev/mysql-server/latest/PAGE_EXTENDING.html ·
bugs.mysql.com/bug.php?id=120427 (2026-05-08) · redis.io/docs/latest/develop/reference/modules · github.com/redis/redis/src/redismodule.h ·
valkey.io/topics/modules-intro · valkey.io/topics/migration · docs.singlestore.com/cloud/reference/code-engine-powered-by-wasm (2025-06-13) ·
cloud.google.com/blog/products/data-analytics/bigquery-supports-apache-datasketches (2025-05-22) · docs.firebolt.io/reference-sql/commands/data-definition/create-function ·
github.com/extism/extism/issues/666 (2024-07-25) · helm.sh/community/hips/hip-0026 (2025) · rustic-ai.github.io/uni-db/plugins/loaders ·
code.visualstudio.com/api/references/activation-events · code.visualstudio.com/API/advanced-topics/extension-host · vldb.org/pvldb/vol18/p1962-kim.pdf (2025) ·
github.com/open-telemetry/opentelemetry-collector/docs/component-stability.md · envoyproxy.io/docs/envoy/latest/configuration/overview/extension ·
grafana.com/developers/plugin-tools/key-concepts/backend-plugins · cwiki.apache.org KIP-891, KIP-146 · duckdb.org/docs/lts/extensions/versioning_of_extensions · duckdb/duckdb#14992 (2024-11-26)

---

## 4. DuckDB (the anchor case) — 2026-11-01

### What an extension can extend

Two registration surfaces, and they are not equivalent.

**C API (in-process, "stable"):** scalar, aggregate, table and copy functions; logical
types and casts; replacement scans; config options; log storage; catalog read access;
table-function projection pushdown. Visible as function-pointer slots in
`duckdb_extension.h` (`register_scalar_function`, `register_table_function`,
`register_copy_function`, `register_logical_type`, `register_cast_function`,
`add_replacement_scan`, `register_config_option`, `register_log_storage`,
`client_context_get_catalog`).

**C++ only (`ExtensionLoader`):** window and pragma functions, SQL macros, secret types
and `CreateSecretFunction`, collations, coordinate systems, combine-types rules, profiler
metrics. No window/pragma/secret/collation symbols exist in `duckdb.h` at all.

**Structural hooks are C++-only:** `StorageExtension::Register` (storage + catalog
backends — DuckLake registers this way), `ParserExtension`, `OptimizerExtension`,
`OperatorExtension`. They need `DBConfig`, which the C ABI never exposes. DuckDB's own v2
C API announcement confirms statistics, optimizer extensions, custom operators, custom
client/instance state and secrets remain outside the C API (discussion #27525,
2026-10-20).

Autoload triggers live in one central table (`EXTENSION_SETTINGS`,
`EXTENSION_FILE_PREFIXES`, `EXTENSION_COPY_FUNCTIONS`, … in `extension_entries.hpp`) plus
a 28-entry `AUTOLOADABLE_EXTENSIONS` allowlist — that is why `FROM 'x.parquet'` pulls in
parquet and a community extension never autoloads.

### The C extension API

`duckdb.h` (client surface) plus the autogenerated `duckdb_extension.h`, which declares
`DUCKDB_EXTENSION_API_VERSION_{MAJOR,MINOR,PATCH}` = **1.5.6**, a `duckdb_ext_api_v1`
struct of **548 function pointers**, `DUCKDB_EXTENSION_ENTRYPOINT` (emits
`<name>_init_c_api`) and `DuckDBExtensionLoadState`. The extension never resolves DuckDB
symbols: the loader hands it `access->get_api(info, "v1.5.6")` and the macro rewrites
every `duckdb_*` call to `duckdb_ext_api.duckdb_*`. The header still carries "WARNING:
this API is not yet stable, meaning it is only guaranteed to work for this specific
DuckDB version" — a leftover from before the stable band existed.

**The append-only claim is only true of the declared-stable prefix.** Header diff:
v1.2.2 (408 slots) → v2.0.0 (548) differs in exactly two slots (`varint`→`bignum` renames
at indices 114 and 138, layout preserved); but v1.4.0 (459) → v1.5.5 (546) *inserts*
`duckdb_appender_clear` at index 410, shifting everything after it.

**Footer.** 512 bytes appended to the shared library: eight 32-byte fields (magic `"4"`,
platform, DuckDB-version-or-C-API-version, extension version, ABI type
`CPP`/`C_STRUCT`/`C_STRUCT_UNSTABLE`, three unused) and a 256-byte signature slot. The
signature is RSA-2048 over a two-level SHA-256 of 1 MiB chunks, covering header+metadata;
verified at install *and* every load. `strip` destroys the footer and DuckDB refuses the
file.

### Loading and distribution

`INSTALL` downloads, verifies and writes to
`~/.duckdb/extensions/<duckdb_version>/<platform>/`; `LOAD` `dlopen`s with
`RTLD_NOW | RTLD_LOCAL` and `dlsym`s the entrypoint. Dispatch by ABI type: `CPP` →
`<base>_duckdb_cpp_init`, `C_STRUCT` → `<base>_init_c_api`, `C_STRUCT_UNSTABLE`/v2 →
`<base>_init_c_api_v2`.

Repositories: `core`, `core_nightly`, `community` (plus backup hosts). DuckDB 2.0 adds
custom repositories: `CREATE EXTENSION REPOSITORY … WITH PREFIX …`, keys pinned from
`/.well-known/duckdb-extension-repo.json` and printed as SHA-256 fingerprints, gated by
`allow_extension_repositories` (`undecided` default). Unsigned extensions need
`allow_unsigned_extensions` / `duckdb -unsigned`, settable only at startup;
`allow_community_extensions = false` narrows trust to core keys.

Templates: `extension-template` (C++, built inside DuckDB's CMake, links DuckDB into the
binary), `extension-template-c` (C API, no DuckDB build), `extension-template-rs`.

### Rust

`duckdb-rs`'s `duckdb` crate is a *client* binding, not the extension path. The official
Rust route is `extension-template-rs`: a `cdylib` on **duckdb-neo 0.20000.1**
("ergonomic wrapper for DuckDB's v2 C API (experimental)") plus
`duckdb-loadable-macros`' `#[duckdb_entrypoint_c_api_v2(extension_name = …,
api_version = "v2.0.0")]`, then `append_extension_metadata.py --abi-type
C_STRUCT_UNSTABLE`. A third-party SDK, `quack-rs`, wraps the v1 C API instead.

Verified from the template repo itself (README, Makefile): the Cargo workspace is
`crates/core` (implementations + registration), `crates/native` (cdylib with the
entrypoint) and `crates/wasm` (a `staticlib` with the *same* entrypoint, built with
`make wasm_eh` into `*.duckdb_extension.wasm` for Node/browser); the build is
`make configure` (Python venv with DuckDB + its sqllogictest runner) → `make debug` →
cargo → a script appends the footer; local loading needs `duckdb -unsigned`. The
Makefile states the coupling plainly: *"Binaries load only in the exact
`TARGET_DUCKDB_VERSION`, not in newer DuckDB releases"* (`TARGET_DUCKDB_VERSION=v2.0.0`).
The template demonstrates scalar functions with init state, table functions with bind
data and global/local state, aggregates (init/update/combine/finalize), chunked table
functions with **projection and filter pushdown, cardinality and progress**, and a
**custom type with CAST/TRY_CAST**.

Pitfalls: a Rust panic surfaces as an internal error that **invalidates the database** for
every later query (catch it yourself); the entrypoint symbol is derived from the file
name, so `<name>.duckdb_extension` must match `DUCKDB_EXTENSION_NAME`; the `-dv` footer
stamp must be the C API version (`v1.2.0`/`v1.5.6`), not the DuckDB release, or `LOAD`
refuses it; Windows + Python 3.11 produced "module could not be found" in the template's
own README.

### Versioning and trust

Extension semver has three tiers: git hash (unstable), `v0.y.z` (pre-release), `vX.y.z`
(stable). Separately, the *extension API version* bumps whenever new functions are
stabilized in the struct — "the struct can only grow but never be modified". `C_STRUCT`
means "minimum API version": 1.2.0-stamped binaries load into every later release;
1.5.6-stamped ones are refused by 1.2.0–1.5.5. The bump is a one-way ratchet — new
extensions stop working on old DuckDB, old ones keep working on new DuckDB. Most of the
ecosystem sat on `v1.2.0` from Dec 2024 to Sep 2026, when 1.5.6 froze the whole struct.
C++ extensions get none of this: they link DuckDB and are pinned to one release. C API v2
(`duckdb_extension_v2.h`, YAML-specified, stability-tagged) shipped experimental in
2.0.0 (2026-10-21) with a C++ wrapper that compiles into the extension and talks only to
the stable C ABI.

### Limits and maintenance lessons

- **No unload, no reload, no uninstall command** — updating extensions requires a process
  restart.
- **No sandbox.** Extensions run with the host process's privileges; signing proves CI
  built it, not that it is safe.
- **Behavior-modifying extensions can't autoload** — autoload is restricted to the core
  allowlist.
- **Community extensions track only the latest stable DuckDB**; older releases see them
  frozen, and there is no way to install a historical community version. Descriptors pin
  a commit per codename (`ref`, `ref_next`).
- **Security settings are one-way** for the session, and `allow_unsigned_extensions` is
  process-wide.
- **Ecosystem cost:** 395 community extensions as of 2026-11-01, each rebuilt by DuckDB's
  CI on every release, with a manual branch-and-`ref_next` dance when the C++ API moves.
- **Design lesson:** the stable unit is a function-pointer vtable, so anything needing new
  *structure* (storage, parser, optimizer, secrets) stays C++-only and version-locked.
  That is the deliberate boundary: DuckDB buys "build once" only for the parts it can
  express as append-only function slots.

**Sources:** github.com/duckdb/duckdb — src/include/duckdb/main/extension/extension_loader.hpp,
src/include/duckdb/main/extension_entries.hpp, src/main/extension/{extension_load.cpp,extension_load_dynamic.cpp} ·
github.com/duckdb/ducklake — src/ducklake_extension.cpp · duckdb.org/docs/2.0/extensions/{versioning_of_extensions,extension_distribution,installing_extensions} ·
duckdb.org/docs/2.0/operations_manual/securing_duckdb/securing_extensions.html · duckdb.org/2026/08/17/duckdb-20-highlights.html (2.0.0, 2026-10-21) ·
github.com/duckdb/duckdb/discussions/27525 (2026-10-20) · github.com/duckdb/duckdb/pull/14992 · github.com/duckdb/duckdb/releases/tag/v1.5.6 ·
github.com/duckdb/extension-ci-tools — scripts/append_extension_metadata.py · github.com/duckdb/extension-template-rs (README, Makefile) ·
github.com/duckdb/duckdb-rs — crates/duckdb-neo/extensions.md · github.com/duckdb/reference-extension-c · github.com/duckdb/community-extensions — UPDATING.md
