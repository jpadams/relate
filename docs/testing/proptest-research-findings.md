# Property-Based Testing in Rust (as of 2026-09) — Research Findings

Scope: proptest / quickcheck / hypothesis-style tooling / cargo-fuzz, best practices
(stateful/model-based, metamorphic), and concrete properties for an in-memory property
graph engine with Cypher + WAL + Parquet checkpoints.

All version numbers and dates are as reported by crates.io / docs.rs / project pages at
the time of research (2026-09-28).

> **Historical note:** the workspace `rand = "0.8"` statements in this
> document are historical as of 2026-09. The workspace migrated to rand 0.9;
> see ADR-0360.

---

## 0. TL;DR for Relate

- **Use `proptest` as the primary harness.** It is the only mature Rust library with
  integrated shrinking, per-type strategy composition, and persisted regression files.
  Latest **1.11.0 (2026-03-24)**, MSRV 1.84 (unreleased bumps to 1.88).
- **`quickcheck` 1.1.0 (2026-02-10)** is fine for trivial `Arbitrary` properties but has
  no regression persistence and weaker shrinking; keep it out unless a transient need.
- **`proptest-state-machine` 0.8.0** for stateful tests (WAL apply / graph mutation
  sequences). Readyset's **`proptest-stateful` 0.1.3** is an async alternative but stale
  (2023-07) and pulls tokio.
- **`cargo-fuzz` 0.13.2 (2026-06-09) + `arbitrary` 1.4.2** for coverage-guided fuzzing of
  the lexer/parser and of WAL/Parquet decoders — complementary to proptest, not a
  replacement (nightly + Unix-only).
- **Dependency note:** the Relate workspace pins **`rand = "0.8"`** (root `Cargo.toml`
  line 118). proptest 1.11 requires **rand 0.9** (`rand_chacha 0.9`, `rand_xorshift 0.4`)
  and the unreleased proptest moves to **rand 0.10**. Cargo will link a *second* rand
  major alongside 0.8 — no compile conflict, but it enlarges the graph and means the two
  rand families must not be mixed in one RNG path.
- **Properties that pay off here:** parser round-trip, WAL replay determinism/idempotency,
  checkpoint-reload equivalence, graph-structure invariants, arithmetic/sorting laws,
  temporal ordering.

---

## 1. The QASkills 2026 proptest guide

Source: <https://qaskills.sh/blog/rust-proptest-property-testing-guide-2026>

### Versions covered
- **proptest 1.11** — "the current stable release in 2026"; pure Rust, no external test
  runner. Installed as `proptest = "1.11"` (or `cargo add --dev proptest`). Works on
  stable and in `no_std` via the `alloc` feature.
- **proptest-derive 0.6** — companion crate for `#[derive(Arbitrary)]`.
- Ecosystem: >140M crates.io downloads, maintained under the `proptest-rs` org; shrinking
  design borrowed from Python's Hypothesis (not classic QuickCheck).
- Optional features mentioned: `timeout`, `fork`.

### Code examples (verbatim shapes from the article)
Commutativity, the canonical first property:

```rust
use proptest::prelude::*;

fn add(a: i32, b: i32) -> i32 { a.wrapping_add(b) }

proptest! {
    fn add_is_commutative(a in any::<i32>(), b in any::<i32>()) {
        prop_assert_eq!(add(a, b), add(b, a));
    }
}
```

Round-trip (generate `u64` → text → parse → equal). Strategy building blocks cited:
- range: `n in 0..100i32` (half-open, like Rust ranges)
- regex string: `s in "[a-z]{1,8}"` (a string literal in strategy position compiles as a regex)
- collection: `v in prop::collection::vec(any::<bool>(), 0..=10)`

Combinators: `.prop_map(f)`, `.prop_filter("reason", pred)`, `.prop_flat_map(f)` (dependent
generation), `prop_oneof![a, b, c]` (uniform choice). `Just(x)` always yields one value.

Derive example: `User { id: u32, age: u8 (#[proptest(strategy = "1..=120u8")]), name: String
(#[proptest(regex = "[a-z]{3,12}")]) }`. Derived enums weight variants equally; adjust via
`#[proptest(weight = N)]`.

### Shrinking
On failure proptest iterates toward simpler inputs (long vectors → `[0]`, large ints → 0).
Article's worked example (indexing `xs[0]` over `vec(any::<i32>(), 0..50)`) reported:
`Test failed: index out of bounds: the len is 0 but the index is 0.` and
`minimal failing input: xs = []`.

### Regression files
Failing seeds persist to **`.proptest-regressions`** and are replayed before random cases on
later runs. Sample entry: `cc 1a2b3c4d... # shrinks to xs = []`. Guidance: **commit the
file** so each counterexample becomes a permanent regression test. (quickcheck has no
built-in persistence.)

### ProptestConfig defaults (article table)
| Setting | Default | Purpose |
|---|---|---|
| `cases` | 256 | random inputs per test |
| `max_shrink_iters` | 1024 (article) / `u32::MAX` (docs) | shrink-step cap |
| `max_global_rejects` | 65536 (article) / 1024 (docs) | allowed `prop_filter`/`prop_assume!` rejections |
| `timeout` | 0 (off) | per-case limit ms; needs `timeout` feature |
| `fork` | false | forked process per case |

Configured inline with `#![proptest_config(ProptestConfig { ... })]` at the top of the
`proptest!` block. (Note: the article's `max_shrink_iters`/`max_global_rejects` defaults
disagree with the current docs.rs page — see §2; trust docs.rs.)

### Filtering / assumptions
`prop_assume!(b != 0);` discards irrelevant cases; cleaner than `.prop_filter` when the
rejection depends on several params. Excess rejection triggers a "too many global rejects"
abort; fix by constraining the strategy (e.g. `1..i64::MAX`).

### CI
Runtime override without recompiling: **`PROPTEST_CASES=10000 cargo test`**. Recommended:
fast local `cargo test` + heavier scheduled/nightly job with raised case count.

### Best practices (article)
- Test properties, not implementations: round-trips, invariants, oracles, idempotence.
- Always commit `.proptest-regressions`.
- Prefer constrained strategies over `prop_assume!`.
- Keep properties fast; hoist expensive setup or lower `cases`.
- Combine with example-based tests.

**Gap:** the article does **not** cover stateful/model-based testing at all — fill that from
§3 below.

---

## 2. Library comparison

### 2.1 proptest — `1.11.0` (2026-03-24)

Source: <https://docs.rs/crate/proptest/latest>, <https://github.com/proptest-rs/proptest/blob/main/proptest/CHANGELOG.md>,
<https://proptest-rs.github.io/proptest/proptest/vs-quickcheck.html>

**Dependency footprint (docs.rs, 1.11.0).** Normal deps only:
`bitflags ^2.10`, `num-traits ^0.2.15`, `rand ^0.9`, `rand_chacha ^0.9`,
`rand_xorshift ^0.4`, `unarray ^0.1.4`. Optional: `bit-set ^0.8.0`, `bit-vec ^0.8.0`,
`proptest-macro ^0.5.0` (the derive), `regex-syntax ^0.8`, `rusty-fork ^0.3.0`,
`tempfile ^3.0`, `x86 ^0.52.0`. Dev: `regex ^1.0`, `trybuild =1.0.115`.

Assessment: **no heavy deps** — no `syn`/`quote` unless you enable the derive, no async
runtime, no native/C++ build. `regex-syntax` is optional (only for the regex strategy).
`x86` (RDRAND) is optional and only pulled by the randomness feature path. MSRV 1.84;
policy "MSRV will not exceed `<current stable> - 7`". Doc coverage 100%.

**rand version constraint — the one real friction point.** proptest 1.7.0 moved
`rand 0.8 → 0.9`; the unreleased branch moves the rand family to **0.10** and migrates the
internal `TestRng` to rand 0.10's trait/seeding APIs "while preserving seeded behavior."
The Relate workspace declares `rand = "0.8"` at the root and uses it in
`relate-executor`, `relate-datasets`, and (optionally) `relate-cli`. Adding
proptest therefore introduces a second, disjoint rand major. It compiles (Cargo permits
multiple semver-incompatible majors), but:
- you cannot pass a `rand::Rng` (0.8) into a proptest strategy, and vice versa;
- any test that seeds both a proptest generator and engine RNG must keep the two streams
  separate (or bump the workspace to rand 0.9 first).

**Shrinking quality (best in class).** Integrated/semantic shrinking: proptest keeps the
intermediate structure of generated values so it can shrink derived values meaningfully.
QuickCheck shrinks "based on the output value" and type only. Cost: proptest "must hold on
to all the intermediate states and relationships"; complex-value generation "can be up to
an order of magnitude slower" than QuickCheck.

**Strategy composition.** Many strategies per type, not one `Arbitrary`+`Shrink` pair.
`prop_map` composes structs from tuples of field strategies; ranges 0..100 vs 0..1000 both
expressible without newtypes; constraints encoded in the strategy avoid rejected inputs
during both generation and shrinking. `prop_flat_map` for dependent generation,
`prop_oneof!` for choice, `prop_filter`/`prop_assume!` for residual predicates.

**Determinism / replay.** Config exposes `rng_algorithm` (`RngAlgorithm::XorShift` = `xs`,
`RngAlgorithm::ChaCha` = `cc`) and `rng_seed` (`RngSeed`), overridable via
`PROPTEST_RNG_ALGORITHM` and **`PROPTEST_RNG_SEED`** (there is **no** `PROPTEST_SEED`).
Persisted failures are **replayed first**, before random cases, and do not count toward
`cases`. With `std` the default persistence is
`FileFailurePersistence::SourceParallel("proptest-regressions")`; disable with
`PROPTEST_DISABLE_FAILURE_PERSISTENCE`. There is **no env var to relocate the persistence
file** (only `Config::with_failure_persistence` in code). All env vars require the `std`
feature (on by default).

**Relevant `ProptestConfig` fields/defaults (docs.rs, 1.11.0):**
| Field | Default | Env |
|---|---|---|
| `cases` | 256 | `PROPTEST_CASES` |
| `max_local_rejects` | 65536 | `PROPTEST_MAX_LOCAL_REJECTS` |
| `max_global_rejects` | 1024 | `PROPTEST_MAX_GLOBAL_REJECTS` |
| `max_flat_map_regens` | 1_000_000 | `PROPTEST_MAX_FLAT_MAP_REGENS` |
| `max_shrink_iters` | `u32::MAX` | `PROPTEST_MAX_SHRINK_ITERS` |
| `max_shrink_time` (std) | 0 (no limit) | `PROPTEST_MAX_SHRINK_TIME` |
| `max_default_size_range` | 100 | `PROPTEST_MAX_DEFAULT_SIZE_RANGE` |
| `fork` (feature) | false | `PROPTEST_FORK` |
| `timeout` (feature) | 0 (off) | `PROPTEST_TIMEOUT` |
| `verbose` (std) | 0 | `PROPTEST_VERBOSE` |
| `rng_algorithm` | default | `PROPTEST_RNG_ALGORITHM` |
| `rng_seed` | random if unset | `PROPTEST_RNG_SEED` |
| `failure_persistence` | file `proptest-regressions` | `PROPTEST_DISABLE_FAILURE_PERSISTENCE` |

Notes: `max_shrink_iters = u32::MAX` actually caps at "four times the number of test
cases"; `0` disables shrinking. A non-zero `timeout` runs cases in a subprocess and implies
forking. `verbose=1` logs failures plus state-machine transitions.

**`cargo test` / CI integration.** proptest tests are ordinary `#[test]` functions (the
`proptest!` macro expands to one) — they run under `cargo test`, `cargo nextest`, and any
CI without a special runner. No nightly, no extra process. Case counts scale via env var
(`PROPTEST_CASES`), making nightly "more cases" jobs trivial.

**Version history worth knowing (CHANGELOG):**
- 1.6.0 — `handle-panics` feature; `with_failure_persistence`; regressions replayed newest-first.
- 1.7.0 — `rand 0.8 → 0.9`.
- 1.9.0 — **MSRV 1.82**; `ProptestResultExt` / `prop_assume_ok`; `lazy_static` → `LazyLock`.
- 1.10.0 — **MSRV 1.84**; memory-efficient subset-sampling strategy.
- 1.11.0 — `bits::u128`/`bits::i128`; `f16` support (feature-gated).
- unreleased — **MSRV 1.88**; rand family → 0.10; fixes for `Duration` overflow,
  single-point float ranges (`0.0..=0.0`), RDRAND soundness, `static mut` data race.

### 2.2 quickcheck — `1.1.0` (2026-02-10)

Source: <https://docs.rs/crate/quickcheck/latest>

Deps: `rand ^0.10` (normal); `log ^0.4`, `env_logger ^0.11` (optional). MSRV 1.85.

- One generator/shrinker per type via `Arbitrary`; custom generation needs a newtype + manual
  trait impls; a single global "size" setting can't distinguish ranges.
- Shrinking uses binary search for lists/numbers and is decent for flat types, but it is
  structurally weaker than proptest's integrated shrinking (documented on proptest's own
  comparison page).
- **No persistence of failing seeds.**
- Limits: ≤8 function params; stack-overflow failures aren't caught; no `Coarbitrary`;
  no `Arbitrary` for closures.
- Discarding via `TestResult::discard()`; defaults 100 valid tests / 10,000 max tries, tuned
  by `QuickCheck::tests`/`max_tests` or `QUICKCHECK_TESTS`, `QUICKCHECK_MAX_TESTS`,
  `QUICKCHECK_MIN_TESTS_PASSED`.
- The quickcheck README itself points users to `proptest` "for those frustrated with shrinking".

Verdict: keep for trivial cases only; proptest is the default.

### 2.3 Hypothesis-style / protocol

- **proptest is itself "Hypothesis-like"** (its crates.io description) — integrated
  shrinking and per-type strategies are the Hypothesis ideas ported to Rust.
- **hegel-rust / `hegeltest` 0.47.4 (2026-09-25)** — new, actively developed (authors
  DRMacIver & Liam-DeVoe, i.e. Hypothesis lineage), "Property-based testing for Rust, built
  on Hypothesis" using the cross-language **Hegel protocol** (<https://hegel.dev/>). In
  **beta**, breaking changes possible. Deps: `hegeltest-macros =0.47.4`, `parking_lot ^0.12`;
  optional `chrono`, `jiff`, `rand ^0.10`, `serde_json`, `hegeltest-c`. Usage:

  ```rust
  use hegel::{generators as gs, TestCase};

  #[hegel::test]
  fn sort_dedups(tc: TestCase) {
      let mut v = tc.draw(gs::vecs(gs::integers::<i32>()));
      let expected = my_sort(&v);
      v.sort();
      assert_eq!(v, expected); // shrinks to a single element
  }
  ```

  Interesting because shrinking is delegated to the Hypothesis engine over a protocol, and
  it is language-agnostic — but it is beta and adds an external process, so **not**
  recommended as Relate's primary harness yet.

### 2.4 cargo-fuzz + arbitrary (fuzzing, not PBT)

- **cargo-fuzz 0.13.2 (2026-06-09)** — `cargo install cargo-fuzz`. Requires a **nightly**
  compiler (unstable flags), LLVM sanitizer support (**x86-64 / AArch64, Unix only, no
  Windows**), and a C++11 compiler. Deps: `anyhow`, `cargo_metadata`, `clap`, `rayon`,
  `tempfile`, ...
- **arbitrary 1.4.2 (2025-08-14)** — turns raw fuzzer bytes into typed values; `"derive"`
  feature for `#[derive(Arbitrary)]`. MSRV 1.63.
- Guidance (<https://rust-fuzz.github.io/book/cargo-fuzz/structure-aware-fuzzing.html>):
  derive `Arbitrary` for AST/input types so libFuzzer can reach structured inputs;
  `fuzz_target!(|data: &[u8]| { if let Ok(t) = <T as Arbitrary>::arbitrary(&mut Unstructured::new(data)) { ... } })`.
- **Where fuzzing fits vs proptest:** fuzzing is coverage-guided and needs nightly + Unix;
  proptest is deterministic, shrink-friendly, `cargo test`-native. Use proptest for the
  merge gate, cargo-fuzz as an occasional deep hunt on the lexer/parser and binary decoders.

---

## 3. Best practices: stateful, model-based, metamorphic

### 3.1 proptest-state-machine — `0.8.0`

Source: <https://proptest-rs.github.io/proptest/proptest/state-machine.html>,
<https://docs.rs/proptest-state-machine/latest/proptest_state_machine/>

Deps: `proptest ^1.10.0` only. Checks a SUT against an abstract reference machine, then
shrinks a failing run "to a minimal sequence of transitions that still reproduce the issue."
Shrinking order: delete transitions from the back to `min_size`, then shrink individual
transitions front-to-back, then shrink the initial state.

**`ReferenceStateMachine`** (associated types `State`, `Transition`):
```rust
fn init_state() -> BoxedStrategy<Self::State>;
fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition>;
fn apply(mut state: Self::State, transition: &Self::Transition) -> Self::State;
fn preconditions(state: &Self::State, transition: &Self::Transition) -> bool { true } // optional
```

**`StateMachineTest`** (associated types `SystemUnderTest`, `Reference`):
```rust
fn init_test(ref_state: &Self::Reference::State) -> Self::SystemUnderTest;
fn apply(mut state: Self::SystemUnderTest,
         ref_state: &Self::Reference::State,
         transition: Transition) -> Self::SystemUnderTest;  // post-conditions asserted here
fn check_invariants(state: &Self::SystemUnderTest, ref_state: &Self::Reference::State);
fn teardown(state: Self::SystemUnderTest) { } // optional
```

Runner macro (only code sample the book gives):
```rust
prop_state_machine! {
    #[test]
    fn name_of_the_test(sequential 1..20 => MyStateMachineTest);
}
```

Tips: raise `cases` (or `PROPTEST_CASES`), set `verbose=1` (`PROPTEST_VERBOSE`) to print
transitions, use `timeout`/`PROPTEST_TIMEOUT` for slow tests. Currently **sequential only**
(concurrent strategy is planned); no symbolic variables; post-conditions live inside
`StateMachineTest::apply`. Inspired by Erlang `eqc_statem` / PULSE. Examples:
`state_machine_heap.rs`, `state_machine_echo_server.rs`.

### 3.2 proptest-stateful (ReadySet) — `0.1.3` (2023-07-03)

Source: <https://github.com/readysettech/readyset/blob/main/proptest-stateful/README.md>,
<https://readyset.io/blog/stateful-property-testing-in-rust>

Deps: `async-trait ^0.1`, `proptest ^1.0.0`, `rand ^0.8.5`, `tokio ^1.28`. **Async-only**,
stale. Trait `ModelState` with `op_generators`, `preconditions_met`, `next_state`,
`init_test_run`, `run_op`, `check_postconditions`, `clean_up_test_run`; run via
`proptest_stateful::test::<TestState>(config)` with `ProptestStatefulConfig { min_ops,
max_ops, test_case_timeout, proptest_config }`.

**Real bugs found in ReadySet's SQL cache engine** (good evidence for databases):
1. Column rename + view — overly broad filter skipped replicating column changes; fix a
   one-liner; 13 steps shrank to 3.
2. Dropped DELETE — a delete coinciding with `ALTER TABLE` on another table was silently
   discarded (no server-side error); caught only by postcondition checks.
3. ENUM crash — create enum, use in table, drop table, alter type → invalidate a
   nonexistent cache. Reproduced within minutes of adding enum support.

Lessons: check expected state after **every** step (catches silent bugs); shrinking turns
opaque failures into root causes; generated sequences beat enumerating edge cases.

Note: uses `rand ^0.8.5`, which matches Relate's current pin — but it is old and
async/tokio-bound. Prefer `proptest-state-machine` (0.8.0) for a sync engine.

### 3.3 Storage-engine examples (real)

**sled (embedded KV store).** Documented testing strategies
(<https://docs.rs/sled/latest/src/sled/doc/testing_strategies/mod.rs.html>):
- **quickcheck-based model testing** on `Tree`, `PageCache`, and `Log` (quickcheck 0.9.2 in
  the crate's dep list), i.e. model-based property testing.
- **proptest-based model testing** via the `model` crate on `PageTable`, plus
  linearizability testing.
- **Failpoints** combined with model testing: "at each IO operation, a test can simulate a
  crash."
- **Crash testing:** processes `kill -9`'d during writes; recovery must replay in order and
  halt "at the first torn log message or incomplete segment."
- libFuzzer operation-sequence fuzzing; TSan/LSan; realtime-priority deterministic
  concurrency; TLA+ for some algorithms.

**sled crash-recovery, concretely, via `proptest-lockstep`**
(<https://docs.rs/crate/proptest-lockstep/latest/source/examples/sled_crash_recovery.rs>):
- SUT wrapper `SledStore` exposes `insert/get/remove/contains_key/len/flush`; `recover()`
  flushes, drops the DB to release the lock, then reopens from disk.
- Model `SledModel` wraps a `BTreeMap<String,String>` ("ordered like sled").
- Actions declared with `#[proptest_lockstep::lockstep_actions(state = SledModel)]`:
  `Insert, Get, Remove, ContainsKey, Len, Flush`, each with a `real_return`.
- Two interpreters (`DbModelInterp`, `DbSutInterp`) run the same action against model and
  SUT; returns are compared (lockstep). Generation biases toward existing keys once the
  model is non-empty.
- `CrashRecoveryModel` with `DurableState = BTreeMap` — `model_checkpoint` clones the full
  data because "the durable state IS the full state"; `sut_recover` calls `recover()`.
- Tests: `run_lockstep_test::<SledLockstep>(1..20)` and `run_crash_recovery_test` with
  `CrashRecoveryConfig { crash_probability: 0.15, max_crashes: 3, cases: 50, ops 5..15 }`.

This is the closest published pattern to what Relate needs (model + SUT lockstep +
injected crash + recover-and-continue).

**TigerBeetle VOPR** (<https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/vopr.md>).
Deterministic simulation: the clock, network, and disk are stubbed; a seed injects faults
(drop/reorder packets, partitions, corrupt disk reads/writes). Determinism keyed on
`(seed, git commit)` so failures replay exactly. Thousands of assertions check invariants
and stay enabled in production ("far better to stop operating than continue in an incorrect
state"); extra checkers verify data files are **byte-for-byte identical across caught-up
nodes**. "One minute of VOPR time is equivalent to days of real-world testing." Inspired by
FoundationDB / Antithesis. No explicit shrinking (that is where proptest's integrated
shrinking complements DST).

**AletheiaDB recovery property tests**
(<https://github.com/autumn-foundation/AletheiaDB/blob/ed94af01/tests/recovery/property_based_tests.rs>).
Uses **proptest** to assert four post-recovery invariants on a WAL + checkpoint system:
1. **Temporal consistency** — transaction timestamps strictly monotonic; valid-time start ≤
   end; deletes close prior versions.
2. **Version-chain integrity** — current storage matches latest version state.
3. **Referential integrity** — every edge references an existing node or tombstone.
4. **ID uniqueness** — no duplicate node/edge/version identifiers post-recovery.

Harness: `RecoveryTestHarness { tempdir, wal_dir, checkpoint_dir }`; `execute_operations`
appends `WalOperation` variants then `wal.flush()`; `recover` builds
`CheckpointConfig::with_data_dir(...)` + `CheckpointManager::recover(wal)`. Config: 128
cases per invariant (64 for the combined), 20–80 ops/case. **Determinism of the test itself
matters:** it uses "deterministic timestamps (base + offset) rather than `time::now()`
to ensure reproducible test failures and consistent temporal ordering" — directly relevant
to Relate property tests. Failure via `TestCaseError::fail("Duplicate version ID ...")`.

**Parquet round-trip (Polars / arrow-rs).** Polars PR #22608 (2025-05-05) added `proptest`
tests for the harder parquet decoding kernels; the author reported proptest was much faster
than Python/Hypothesis, strategies generalize "quite easily ... for all our arrow array
types," and it avoids cargo-fuzz's downsides (no exposing internals, no extra CI step)
(<https://github.com/pola-rs/polars/pull/22608>). arrow-rs tracks the same idea: round-trip
Arrow ↔ Parquet for every supported `DataType`, with dedicated bug fixes for `Decimal256`
and a known "RecordBatch with no columns cannot be round-tripped" issue — i.e. round-trip
properties find real codec bugs
(<https://github.com/apache/arrow-rs/issues/1053>, <https://github.com/apache/arrow-rs/issues/6988>).

**Turso differential oracle fuzzing** (query engine, cargo-based, not proptest)
(<https://github.com/tursodatabase/turso/blob/4e4d2bf3/testing/differential-oracle/fuzzer/printf_fuzzer.rs>).
Generates random `SELECT printf(...)` statements from a seeded `ChaCha8Rng`, executes on
both Turso and SQLite, and compares results with an oracle that tolerates float differences
(`floats_close` relative tolerance `1e-5`, `fuzzy_text_eq` parses diverging numeric spans,
`rows_fuzzy_eq` retries with spaces stripped). Failures print a reproduce command
(`cargo run --bin printf_fuzzer -- --seed {} -n {} --verbose`). This is the **differential**
oracle pattern (same query, two engines) that Relate could apply against Neo4j or
against `RELATE_PARSER=planner-ir`.

### 3.4 Metamorphic / oracle testing (guidance)

Source (<https://github.com/shenli/distributed-system-testing>, crash-recovery reference).

- **Durability:** no acknowledged write disappears after a crash; watch fsync gaps,
  partial-checkpoint corruption, WAL-truncation races, orphaned temp files.
- **Replay idempotency:** catch lost-writes-after-crash, double-apply-on-replay, duplicate
  side effects from retries.
- **Snapshot equivalence:** after recovery, state must be equivalent to a known-good
  baseline via **replay equivalence**.
- **Upgrade compatibility:** mixed-version state at every intermediate step, not just N→N+1.
- **Fault injection:** enumerate operation IO boundaries and crash at each one (matrix =
  crash-at-every-boundary × every upgrade step); drive deliberate retry storms for
  idempotent ops; treat fsync as an explicit contract.
- Named tools: ALICE (application-level crash explorer), Torturing Databases framework,
  CrashMonkey (ext4). No code — translate into your own harness.

---

## 4. Properties for an in-memory property graph engine (Cypher + WAL + Parquet)

Concrete, testable properties, grouped by subsystem. Each is a candidate `proptest!`
property; library/API references are in §2–3.

### 4.1 Parser / front-end round-trip
- **AST → Cypher text → AST** is identity: `parse(print(ast)) == ast` (modulo normalized
  whitespace/quoting). This is the classic round-trip and finds printer/parser asymmetries.
- **Text → AST → text → AST** stable after one normalization pass (idempotent printing):
  `normalize(parse(print(parse(s)))) == normalize(parse(print(parse(s))))`-style.
- **Errors are total:** any byte string either parses or returns a typed `EngineError`
  (never panics). Generate arbitrary `String`/`Vec<u8>` and assert no panic — a cheap
  proptest property that also runs well under cargo-fuzz for coverage.
- **Regex/string-literal strategy** for identifiers: `"[a-zA-Z_][a-zA-Z0-9_]{0,20}"` so the
  generator emits valid identifiers, avoiding mass rejection.
- **Statement splitting** (`relate-splitter`): for any multi-statement string,
  `split(s)` concatenated reconstructs `s`; splitting is idempotent; incomplete input is
  reported consistently.

### 4.2 Expression evaluation laws
Cypher semantics to respect (source: Neo4j Cypher Manual "Ordering, equality, comparison"
<https://neo4j.com/docs/cypher-manual/current/values-and-types/ordering-equality-comparison/>;
openCypher CIP2016-06-14 on orderability/equivalence):
- **Commutativity** (where defined): `a + b == b + a` for equal numeric types;
  `a * b == b * a`; `a AND b == b AND a`; `a OR b == b OR a` (three-valued logic holds).
- **Associativity** (where defined): `(a + b) + c == a + (b + c)` for same-type integers
  **and floats** (floats only up to the model's defined semantics — see the overflow caveat).
- **Identity/annihilator:** `a + 0 == a`; `a * 1 == a`; `a * 0 == 0`; `x OR false == x`;
  `x AND true == x`.
- **Three-valued logic:** `null = x` and `null <> x` are `null` for **any** `x`, including
  `null = null`. Assert this explicitly for every operand type.
- **Integer overflow:** openCypher/Neo4j raise on overflow (ArcadeDB issue #5164 documents
  the wrapping-vs-error divergence). Property: for `a, b: i64`, `checked_add` matches the
  engine result; on overflow the engine returns an error, never a wrapped value. Generate
  near-boundary values (`i64::MIN/2`, `i64::MAX/2`, ±1) with a strategy biased to extremes.
- **Division:** `a / 0` is an error (or documented result); `a / 1 == a`; integer division
  truncation direction consistent.
- **Float specials:** NaN comparisons — "All comparability tests with `Double.NaN` evaluate
  as false"; `NaN` ordering per the manual. Property: `NaN == x` is false for all `x`;
  `NaN = NaN` is `null` under three-valued logic but `NaN < NaN` and `NaN > NaN` are false.
- **String/list/map equality** is structural: lists equal iff same sequence of equal values;
  maps equal iff same keys to equal values; `PATH` equals a list of alternating
  node/rel/element.

### 4.3 Sorting / ordering
- **Ordering is a total preorder** on comparable values of one type: reflexive,
  transitive, antisymmetric-or-indistinguishable.
- **Sort stability:** `ORDER BY` with equal keys preserves input order for stable sorts;
  assert the engine's documented guarantee. Property: `sort(sort(xs)) == sort(xs)` (idempotence).
- **Null ordering:** "null is ordered after all other values" — assert nulls sort last (or
  per the engine's documented default).
- **Type ordering hierarchy:** `MAP < NODE < RELATIONSHIP < LIST < PATH < VECTOR < POINT <
  ZONED DATETIME < ... < STRING < BOOLEAN < UUID < Numbers`. Property: for two generated
  values of different types, the comparator agrees with the documented rank.
- **Durations cannot be compared** (returns `null`) — assert.
- **Distinct / GROUP BY equivalence:** `x = y` never errors on comparable types, and
  grouping by a key collapses exactly values that are `=`-equal (watch NaN and null keys).

### 4.4 Graph structure invariants
Following AletheiaDB's recovery invariants — generalize and add:
- **Referential integrity:** every relationship endpoint resolves to an existing node (or a
  documented tombstone); no dangling edge after any operation sequence.
- **ID uniqueness:** no duplicate node/relationship IDs, live or historical.
- **Cardinality conservation:** after a generated sequence of `CREATE`/`DELETE`,
  `count(nodes) == created - deleted`; `count(rels) == created - deleted`.
- **Label/property index consistency:** for a random graph, every node returned by a label
  scan actually carries the label; every match by indexed property equals the linear scan
  result (index vs. scan **differential oracle** — same query, two plans).
- **Round-trip through the value model:** `CypherValue -> internal repr -> CypherValue` is
  identity for all generated values (including temporal, spatial, null, nested lists/maps).
- **MERGE idempotency:** applying the same `MERGE` twice creates nothing the second time
  and leaves the graph unchanged (compare pre/post store).
- **=`-equality vs identity:** two structurally equal values must be interchangeable as
  map keys / property values in matching.

### 4.5 WAL replay determinism and idempotency
- **Determinism:** replaying the same WAL twice yields byte-identical graph state
  (snapshot both, compare). Assert with a canonical serialization (e.g. sorted node/edge
  dumps) to avoid hash-map iteration-order flakiness.
- **Idempotency:** replaying a WAL record that was already applied is a no-op (or the log
  format guarantees at-most-once by sequence number). Property: `apply(apply(state, r), r) ==
  apply(state, r)`.
- **Prefix property:** replaying a prefix of the WAL produces exactly the state at that
  point (`replay(w[0..k])` == state after the k-th operation).
- **Ordering:** recovered state equals the state produced by applying operations in WAL
  order — "Transaction timestamps increase monotonically" (AletheiaDB invariant 1).
- **Truncation safety:** a torn/partial trailing record is either fully rejected on recovery
  or fully ignored — never partially applied (sled: halt "at the first torn log message").
  Property: truncate the WAL byte stream at every offset, recover, assert no panic and a
  consistent state that is a prefix of the full state.
- **Deterministic test timestamps:** generate timestamps as `base + offset`, never
  `now()` — otherwise failures are not reproducible (explicit AletheiaDB practice).

### 4.6 Checkpoint / Parquet reload equivalence
- **Checkpoint reload equivalence:** `load(checkpoint(dump(state))) == state` for all
  generated graphs (the primary property; mirror arrow-rs's per-`DataType` round-trip).
- **WAL+checkpoint equivalence:** `recover(checkpoint, wal_tail) == replay(full_wal)`; i.e.
  replaying from the last checkpoint plus the tail equals a full replay from genesis. This
  is the "snapshot / state equivalence via replay equivalence" property from §3.4.
- **Checkpoint idempotence:** checkpointing twice without intervening writes produces the
  same file/state; checkpointing is a no-op on an empty tail.
- **Plan/schema round-trip:** the stored graph schema (labels, rel types, property keys,
  index definitions) survives checkpoint/reload unchanged.
- **Parquet value fidelity:** every `CypherValue` type (int, float incl. NaN/±Inf, string
  with unicode, list, map, temporal, spatial, null) round-trips through Parquet exactly;
  floats compared with an explicit NaN/Inf-aware equality (mirror Turso's `floats_close`
  only where exactness is genuinely impossible).
- **Crash-in-the-middle:** kill during checkpoint write, reopen, assert the pre-checkpoint
  state is recovered (atomic replace semantics).

### 4.7 Stateful composition (recommended primary harness)
Model the engine as a `ReferenceStateMachine` over an ordered map/adjacency model
(`BTreeMap`-based, like sled's model) with transitions:
`CreateNode(labels, props)`, `CreateRel(type, props)`, `DeleteNode(id)`, `DeleteRel(id)`,
`SetProperty`, `MatchQuery(q)`, `CreateCheckpoint`, `Recover`, `AppendWal`, `TruncateWal`,
`ReloadFromCheckpoint`. `check_invariants` asserts §4.4 after every transition; `apply`
asserts post-conditions (query result equals model evaluation); a `CrashAndRecover`
transition applies the §4.5/§4.6 checks mid-sequence. This directly mirrors the sled
crash-recovery example (§3.3) and Readyset's success at finding real database bugs (§3.2).

```rust
prop_state_machine! {
    #[test]
    fn graph_engine_sequential(sequential 1..30 => GraphEngineTest);
}
```

---

## 5. Practical recommendations for Relate

1. **Harness:** add `proptest = "1.11"` (dev) + `proptest-state-machine = "0.8"` (dev).
   Decide on the `rand` situation first: either accept a second rand major (0.9) alongside
   the workspace's `rand 0.8`, or bump the workspace to `rand 0.9` in a separate change.
2. **Persistence:** commit `.proptest-regressions`; never set
   `PROPTEST_DISABLE_FAILURE_PERSISTENCE` in CI. This is the cheap, high-value half of the
   whole setup.
3. **CI:** default `PROPTEST_CASES=256` locally; add a nightly/scheduled job with
   `PROPTEST_CASES=10000` and `RUST_BACKTRACE=1`. Property tests are ordinary `#[test]`s, so
   they slot into the existing `cargo test` gates and the feature-bundle matrix with no new
   runner. If a property is slow, cap it with a per-test `#![proptest_config(...)]` and
   `PROPTEST_TIMEOUT` rather than dropping coverage.
4. **Avoid over-rejection:** encode constraints in strategies (`0..100i32`, regex literals,
   `prop_oneof!`) instead of `prop_assume!`; keep global rejects low so shrinking stays fast.
5. **Determinism in tests:** generate timestamps/IDs deterministically (base + offset), and
   compare recovered state via a **canonical, order-independent dump** to avoid flakiness
   from hash-map iteration order.
6. **Fuzzing as a supplement:** add a `cargo-fuzz` target for the lexer/parser and the
   WAL/Parquet decoders (no-panic property), run it out-of-band — it needs nightly + Unix
   and is not part of the merge gate.
7. **Differential oracles:** compare query results across plans
   (`RELATE_PARSER=planner-ir` vs default) and against Neo4j/Turso-style references where
   feasible; this is the highest-yield class for query engines (§3.3 Turso, §4.4 index vs
   scan).

---

## 6. Sources

- QASkills guide — <https://qaskills.sh/blog/rust-proptest-property-testing-guide-2026>
- proptest crate/docs — <https://docs.rs/crate/proptest/latest>, <https://crates.io/crates/proptest>
- proptest CHANGELOG — <https://github.com/proptest-rs/proptest/blob/main/proptest/CHANGELOG.md>
- proptest vs quickcheck — <https://proptest-rs.github.io/proptest/proptest/vs-quickcheck.html>
- proptest `Config` — <https://docs.rs/proptest/latest/proptest/test_runner/struct.Config.html>
- proptest shrinking tutorial — <https://proptest-rs.github.io/proptest/proptest/tutorial/shrinking-basics.html>
- proptest state-machine book — <https://proptest-rs.github.io/proptest/proptest/state-machine.html>
- proptest-state-machine docs — <https://docs.rs/proptest-state-machine/latest/proptest_state_machine/>
- quickcheck docs — <https://docs.rs/crate/quickcheck/latest>
- Hegel / hegeltest — <https://github.com/hegeldev/hegel-rust>, <https://docs.rs/crate/hegeltest/latest>, <https://hegel.dev/>
- cargo-fuzz — <https://docs.rs/crate/cargo-fuzz/latest>, <https://github.com/rust-fuzz/cargo-fuzz>
- arbitrary — <https://docs.rs/crate/arbitrary/latest>, <https://rust-fuzz.github.io/book/cargo-fuzz/structure-aware-fuzzing.html>
- proptest-stateful (ReadySet) — <https://github.com/readysettech/readyset/blob/main/proptest-stateful/README.md>, <https://readyset.io/blog/stateful-property-testing-in-rust>
- Nikos Baxevanis, model-based stateful testing — <https://blog.nikosbaxevanis.com/2025/01/10/state-machine-testing-proptest/>
- sled testing strategies — <https://docs.rs/sled/latest/src/sled/doc/testing_strategies/mod.rs.html>
- proptest-lockstep sled crash-recovery example — <https://docs.rs/crate/proptest-lockstep/latest/source/examples/sled_crash_recovery.rs>
- TigerBeetle VOPR — <https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/vopr.md>
- AletheiaDB recovery property tests — <https://github.com/autumn-foundation/AletheiaDB/blob/ed94af01/tests/recovery/property_based_tests.rs>
- Polars proptest parquet kernels — <https://github.com/pola-rs/polars/pull/22608>
- arrow-rs parquet round-trip issues — <https://github.com/apache/arrow-rs/issues/1053>, <https://github.com/apache/arrow-rs/issues/6988>
- Turso differential oracle fuzzer — <https://github.com/tursodatabase/turso/blob/4e4d2bf3/testing/differential-oracle/fuzzer/printf_fuzzer.rs>
- Distributed-system testing / crash-recovery reference — <https://github.com/shenli/distributed-system-testing/blob/main/skills/designing-distributed-system-tests/references/crash-recovery-and-upgrade.md>
- Neo4j Cypher ordering/equality/comparison — <https://neo4j.com/docs/cypher-manual/current/values-and-types/ordering-equality-comparison/>
- openCypher CIP2016-06-14 — <https://github.com/opencypher/openCypher/blob/master/cip/1.accepted/CIP2016-06-14-Define-comparability-and-equality-as-well-as-orderability-and-equivalence.adoc>
