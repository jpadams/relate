# Relate proptest roadmap — the rest of the property catalog

Source: `proptest-research-findings.md` §4 (property catalog). Phase 1 (`wal-proptest`, in
flight) covers the storage layer. The catalog splits into four further features, each its
own gbuild graph on a stacked branch (`main → feat-proptest → feat-proptest-<slug>`, one PR
each, rebase never merge).

All features share conventions established in phase 1: proptest dev-deps per crate, shared
`tests/proptest_support/` harness per crate, `#[proptest]` fn per property, committed
`.proptest-regressions`, PR-level default 256 cases + nightly elevated job.

## Phase 2 — `parser-proptest` (relate-parser + relate-splitter)

Destination: the parser never panics on arbitrary input and round-trips valid statements.

- **No-panic / typed-error**: any byte string either parses or returns a typed
  `EngineError` (never panic, never hang). Strategy: arbitrary bytes + unicode strings
  weighted toward Cypher-shaped tokens.
- **Round-trip**: `parse(print(ast)) == ast` modulo normalization — requires a pretty
  printer or canonical AST comparison; if no printer exists, use structural AST equality
  after re-parse.
- **Statement splitting completeness** (relate-splitter): any multi-statement string
  splits into parts that each parse, and concatenation (with `;`) re-parses to the same
  statement sequence. Drives `is_complete`/input-completeness API.

Effort: smallest of the four; good shakedown for the parser crate harness.

## Phase 3 — `eval-proptest` (relate-executor eval/)

Destination: expression evaluation obeys openCypher semantics laws under generated values.

- **Algebra laws**: `a + b == b + a`, `(a + b) + c == a + (b + c)` (same-type integers),
  identity elements (`a + 0 == a`, `a * 1 == a`, `x OR false == x`).
- **Three-valued logic**: `null = x` and `null <> x` are `null` for ANY x (incl. nested
  lists/maps, NaN).
- **Overflow raises**: i64 arithmetic overflow raises (openCypher/Neo4j semantics — cite
  ArcadeDB issue #5164 precedent from findings).
- **Division**: `a / 0` documented result; `a / 1 == a`; integer division semantics.
- **NaN comparability**: all comparability tests with NaN evaluate false (or per
  documented engine semantics); NaN tagged handling consistent with the harness's
  property_value strategy.
- **Equality/ORDER semantics**: lists equal iff same sequence; ORDER BY with equal keys
  preserves input order (stability); nulls ordered after all other values; full type
  ordering hierarchy `MAP < NODE < RELATIONSHIP < LIST < PATH < VECTOR < POINT < ...`
  (Neo4j order per findings) as a total-order property: any two unequal values have a
  consistent, antisymmetric, transitive comparison.

Effort: medium — largest surface; split expression laws from ORDER BY into separate
nodes in the graph (independent test files, same wave).

## Phase 4 — `graph-proptest` (relate-executor graph/, stateful)

Destination: graph mutations preserve structural invariants under random op sequences.

- **Stateful model** (`prop_state_machine!`): transitions = CREATE node/rel, SET/REMOVE
  props, AddLabels/RemoveLabels, DELETE node/rel, MERGE on key — reference model tracks
  expected node/rel/prop state; compare after each transition (extends the phase-1 WAL
  model to the executor layer).
- **Referential integrity**: every relationship endpoint resolves to an existing node —
  checked after every generated transition sequence, including DELETE ordering.
- **ID uniqueness**: no duplicate node/rel IDs, live or historical.
- **MERGE idempotency**: MERGE on the same key twice yields one node, same props.
- **Index-vs-scan oracle**: with a uniqueness constraint present, index lookup returns
  exactly the node set a full scan returns (addresses the quadratic-scan observation in
  INSTRUCTIONS: constraint-owned index not serving per-row lookup).

Effort: largest; the reference model is the hard part.

## Phase 5 — cross-layer metamorphic (optional, decide after phase 3)

- Query-result vs redo-log consistency: after a query-engine run, applying the captured
  RedoOp sequence to a fresh Database yields the same final state (bridges executor and
  storage).
- WAL ⊂ executor property reuse: phase-1 harness's RedoOp strategy drives executor graph
  application — the two models must agree.

## Sequencing

```
wal-proptest (phase 1, running)
  -> parser-proptest (phase 2)   [independent of 3/4; could parallel]
  -> eval-proptest (phase 3)
  -> graph-proptest (phase 4)   [reuses phase-1 RedoOp strategy + phase-3 value strategy]
  -> cross-layer (phase 5, optional)
```

Phases 2–4 are mutually independent (different crates); stack them in this order for PR
reviewability, or branch 2 and 3 in parallel off phase 1. Phase 4 reuses the phase-1
harness strategies, so it must stack after phase 1 merges.

## rand note

The migration to a single rand 0.9 major **landed** and is recorded in
ADR-0360: the workspace declares
`rand = "0.9"` (resolved 0.9.5), so proptest's rand 0.9 and the workspace share
one RNG major. The wasm32 wiring is resolved: it needs only the Cargo
`wasm_js` feature on getrandom 0.3.4 — no RUSTFLAGS or `.cargo/config.toml`
change exists or is wanted, and the `--cfg getrandom_backend="wasm_js"`
spelling is unnecessary and would override the feature if set. The
frozen-envelope re-freeze stays CI-owned and still pending (ADR-0071; the
current envelopes, measured deltas and measurement environment are
single-sourced in ADR-0360 §4). ADR-0268 §2's `rand 0.8` workspace pin is
superseded by ADR-0360; its proptest adoption stands. R6 does not trigger
(existing dep, major-version bump).

## `float-roundtrip` fix PR (LABS-134) — shipped on `oskar/float-roundtrip`

The follow-up stacked on wal-proptest was implemented and landed on
`oskar/float-roundtrip`:

1. Workspace `Cargo.toml` enables `serde_json`'s `float_roundtrip` feature.
2. The 4 blocked properties were un-`#[ignore]`d (1 in wal_proptest.rs, 3 in
   recovery_proptest.rs) and pass at default and `PROPTEST_CASES=256` case
   counts. (The recovery suite's `cut == 0` empty-checkpoint harness defect,
   which also blocked joint replay but was not part of LABS-134, was already
   fixed on `feat-proptest`.)
3. ADR-0268 carries a `## Later references` note recording that the proptest
   suites found the float codec bug (1-ULP f64 drift through WAL replay and
   checkpoint reload, both VFS backends; serde_json without float_roundtrip is
   not correctly rounded) and that this branch fixed it.
4. The golden-file audit for crates relying on serde_json's lossy parse
   behavior ran green with an empty repair set — no golden relied on the lossy
   parse.

This was the first bug the property suites caught that shipped in the product.

## Non-finite-float persistence contract

The `property_value` float strategy draws `NaN`/`±Infinity`, and the
persistence layer splits on them:

1. **WAL**: round-trips them via the tagged special encodings.
2. **Parquet snapshots**: store them natively.
3. **v2 JSON snapshot codec**: rejects them at encode — `finite_number()` in
   `relate-model/src/serde_value.rs` ("rejecting values that cannot
   round-trip"; ADR-0087 fail-closed, serde_json has no non-finite
   representation).
4. **In-memory write path**: accepts them; the reject is encode-side only.

Consequence for the harness: the checkpoint-bearing properties filter
non-finite floats under the JSON codec; WAL- and parquet-only properties keep
them. A format-level fix (a tagged non-finite encoding in the JSON snapshot)
would land as a `value_encoding` bump per ADR-0120 (`value_encoding: 3` is the
precedent) — recorded here as where it would land, not as a decided change.

## Follow-up 2: rand 0.9 full-migration consequences

Recorded before the 2026-09 revert; resolved by the migration recorded in
ADR-0360 except the CI-owned re-freeze, which is still pending:

1. **Baseline re-freeze (CI-owned, pending)**: rand 0.9 integer range
   sampling differs from 0.8, so generated dataset content and the recorded
   logical bytes change. The current envelopes, this branch's measured
   before/after figures, and the measurement environment are single-sourced
   in ADR-0360 §4 (Decision 4). Re-freeze stays CI-owned (ADR-0071): dispatch
   `.github/workflows/create-loadtest-baselines.yml` from the PR branch (it
   opens an envelope PR based onto that branch); never freeze host-native
   reports. The re-freeze is required for `make gate` and for whenever the
   committed `make compare-loadtests` step in the `loadtest-baselines` job is
   re-enabled — that step is currently commented out.
2. **wasm32 getrandom backend wiring (resolved)**: rand 0.9 pulls getrandom
   0.3.4 through rand_core. `relate/relate-storage/Cargo.toml` pins it
   as `getrandom03` with the Cargo `wasm_js` feature under
   `[target.'cfg(target_arch = "wasm32")'.dependencies]`, beside the existing
   getrandom 0.2 `js` entry (ADR-0085). The mechanism, its 0.3.4 floor and the
   no-RUSTFLAGS consequence are recorded in ADR-0360 §3; the existing wasm CI
   build picks the target-scoped dependency up, so no workflow change was
   needed.
