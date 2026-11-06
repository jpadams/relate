# Extension system — executive summary (2026-11-01)

**Question.** Would a DuckDB-style extension system — one that can extend storage,
engine, Cypher, surfaces and CLI — be worth building for Relate, and how would such
extensions be loaded, given Rust has no stable ABI?

**Recommendation: yes, but tiered — and never one universal plugin ABI.** Four tiers,
each with its own trust and versioning contract, plus one governance layer. Start with the
tier that needs no ABI at all.

| Tier | What it extends | Mechanism | Version coupling | Cost |
| --- | --- | --- | --- | --- |
| **T0** in-tree Rust | storage, engine, Cypher, surfaces | compile-time registry behind cargo features | none | rebuild the binary |
| **T1** C ABI vtable | functions, aggregates, table functions, UDTs, casts | append-only function-pointer struct, signed footer, min-version | min-version ratchet (DuckDB `C_STRUCT`) | per-OS/arch artifacts, CI rebuilds |
| **T2** WASM | untrusted/agent UDFs, loaders | component model (Extism fallback) | WIT/WASI version | ~24 µs per call |
| **T3** out-of-process | command extensions (shipped), surface adapters, CLI verbs | stdio/gRPC child | message schema | 6–20 µs per round trip |

## Why tiered, not unified

- **No database extends storage/parser/engine through a dynamic ABI.** DuckDB keeps
  `StorageExtension`, `ParserExtension`, `OptimizerExtension` **C++-only** and pinned to
  one release — its stable C vtable covers functions, types, scans and pushdown, nothing
  structural. Postgres opens everything and pays with recompiles per major and 16.8%
  pairwise incompatibility across 441 extensions. SQLite closes the parser and planner and
  has the most portable extension ecosystem in existence. ClickHouse refuses plugins
  entirely; Neo4j is JVM-only and calendar-version-locked.
- **Rust has no stable ABI, and the escape hatches are not ready.** `abi_stable` has been
  unmaintained since 2023, `stabby` is SemVer-prime and niche, crABI is an unmerged RFC.
  So "Rust extensions" means a **C ABI boundary with Rust behind it** — which is exactly
  what DuckDB's own Rust template does (`duckdb-neo` over the v2 C API,
  `--abi-type C_STRUCT_UNSTABLE`, exact-version coupled).
- **The shipped command extensions are already the right tier for importers.** T3 gives
  crash isolation, language freedom and schema-based versioning — and it is what
  `relate run` already is.

## What to do first

1. **T0**: a Rust trait registry for procedures/functions behind a cargo feature — the
   Neo4j `CALL` model on the existing procedure registry. No ABI, no new trust surface,
   TCK-gated. Keep the **parser closed**; "extending Cypher" means adding callables and
   preview-gated clauses, never a per-install grammar.
2. **Docs**: add "when to embed, when to extend" to `docs/api/extensions/description.md` —
   the question every author asks.
3. **T1 only after that**, with the three decisions made up front that DuckDB's own
   experience forces: the panic policy (a Rust panic must become a typed refusal, never a
   poisoned store), allocation ownership, and capability probing for forward
   compatibility. Reuse the ADR-0449 trust store and signing.

## What to avoid

Raw Rust `dylib` + Rust types (documented SIGSEGV class); a stable storage ABI before the
on-disk format is frozen; autoloading behavior-modifying extensions; and any claim that
in-process native code is sandboxed — signing proves provenance, not safety.

## The honest cost

An extension ecosystem is a permanent CI and versioning tax: DuckDB rebuilds 395 community
extensions every release and gates them behind an allowlist; Postgres' hooks are
unverifiable in combination. Budget that before opening T1, and treat T3/T0 — which cost
nothing extra to maintain — as the default home for new capability.

## Refinement (2026-11-01)

With an embedded API, the T1/T2 line is **what the plugin touches**, not native-vs-WASM.
Transform-only extensions (UDFs, chunkers, embedders, geo predicates) collapse into T2
once the API is **batch-oriented with opaque handles**: ~24 µs per WASM call ÷ a 1000-row
batch ≈ 24 ns/row, i.e. parity with native — and then T2 wins on portability and sandbox.
T1 survives only for code that holds or mutates host state (storage, indexes, planner
rules, stateful operators) — exactly what DuckDB keeps C++-only. Before any vtable, try
**"T0.5" vendored plugins** (an external crate compiled in behind a cargo feature): no ABI,
no version skew, most of T1's value at zero ABI risk. Python's practical WASM target is
`componentize-py`; Pyodide only for the full scientific stack.

Details: [`extension-system-analysis.md`](extension-system-analysis.md) ·
sources and raw reports: [`extension-system-findings.md`](extension-system-findings.md)
