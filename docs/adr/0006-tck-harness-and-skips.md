# 0006. TCK harness vendors feature files; expanded count is the denominator; skips only shrink

- **Status**: Accepted
- **Date**: 2026-08-28
- **Deciders**: MemoryDB maintainers

## Context

ADR-0056 pins openCypher TCK 2024.3 as the language contract. We need a
reproducible way to run it, report it, and grow coverage without silent
regressions or inflating “pass rate” by skipping hard tests.

## Decision

- Vendor TCK feature files and named graphs under `tests/tck/` (no live
  fetch in CI).
- The compliance denominator is the **cucumber-expanded** scenario count
  (Scenario Outline × Examples rows), published in
  `docs/tck/compliance.md`.
- The harness (`memorydb-tck`) implements TCK Given/When/Then against a
  fresh in-memory database.
- Scenarios that do not yet pass are **explicit skips** with a reason.
  The skip list only shrinks. New skips require an issue or ADR.
- CI fails if a scenario in the expected-pass set fails.
- `TCK_ONLY` may run a subset locally; that run must not rewrite the
  published 100% gate numbers as if the subset were the corpus.

## Alternatives considered

- **No skip list; fail CI on any failure from day one** — blocks all other
  work until ~3900 scenarios pass. Rejected; coverage is climbed
  feature-by-feature.
- **xfail that can grow** — hides regressions and lets pass-rate gaming.
  Rejected; skips only shrink.
- **Depend on cucumber-jvm / Maven TCK runner** — pulls a JVM into a Rust
  engine CI. Rejected; harness is Rust.
- **Re-download TCK each CI run** — pin drift. Rejected; vendor the release.

## Consequences

- `docs/tck/compliance.md` is updated when the allowlist grows.
- TCK mock procedures live in the harness, not in the engine catalog, until
  a procedures ADR says otherwise.
- Parser bugs that break a passing scenario are release blockers.

## Later references

- [ADR-0054](0054-one-c-abi-env-python.md): corpus path is `testdata/tck`.
