# Embedded driver providers — analysis (2026-11-03, LABS-447)

**Question.** Can the stock official Neo4j drivers (Java, Python, JavaScript) run against embedded Relate in-process — Cypher in, driver-native hydrated objects out — with no socket, no Bolt bytes and no PackStream, and with no upstream driver changes?

**Answer in one line.** Yes, in all three languages: each driver has a hydration-level seam a provider can occupy; what limits fidelity is not the drivers but our C ABI, which is JSON-only below the top level of a result and carries no error codes — so every leg needs the same typed-value + error-code ABI extension before a v1 ships.

Raw material: [`embedded-driver-provider-findings.md`](embedded-driver-provider-findings.md)
(the three spike reports verbatim: Java, Python, JavaScript). Spikes only —
nothing merged; branches `spike/labs-447-java-provider`,
`spike/labs-447-python-provider`, `spike/labs-447-js-provider` (local
worktrees). This study feeds LABS-447 (the driver-compatible embedded facade)
and is the evidence base for the typed-value + error-code C ABI ADR.

---

## 1. The three seams

| | Java | Python | JavaScript |
| --- | --- | --- | --- |
| Driver version | neo4j-java-driver 6.3.0 | neo4j 6.4.1 | neo4j-driver 6.3.0 (npm latest) |
| Seam | `BoltConnectionProvider` SPI (bolt-connection 12.1.0, ServiceLoader) | `AsyncConnection`/`Connection` ABC + `AsyncIOPool`/`IOPool` (private) | `ConnectionProvider` + `Connection` in neo4j-driver-core (private, via `Driver` ctor or monkey-patch) |
| Upstream hook? | Yes — the SPI is public but unsupported | Monkey-patch (user steer) | Monkey-patch (user steer); `install()` covers CommonJS + ESM default imports, not ESM named imports |
| Engine access | JSON C ABI via JNA (the fat-jar client) | pyo3 typed binding + small native seam | JSON C ABI via koffi (existing npm client) |
| Wire bypass | full (no PackStream, no channel) | full | full (bolt-connection layer bypassed; its `ResultStreamObserver` reused) |
| Advertised protocol | Bolt 4.4 | protocol (5, 8) with `statuses` | Bolt 4.4 |

## 2. What passes unpatched (all three legs)

`executeQuery`/`execute_query`, managed and explicit transactions, **retried
write-write conflicts** (when the error is classified transient — see §4),
`fetchSize` paging + DISCARD/consume, EXPLAIN/PROFILE plans and notifications,
`database` routing → namespace, file-store open/reopen, and multi-session
concurrency (8 sessions/threads sharing one store, 200/200 increments). Java
adds reactive; Python adds async; JS adds RxSession.

Per-trivial-statement overhead:

| Path | Java | Python | JS |
| --- | --- | --- | --- |
| Client direct | — | 12 µs | 13–16 µs |
| `session.run` | — | 34 µs | 47 µs |
| `executeQuery` | — | 64 µs | 79 µs |

## 3. Value fidelity — the ABI decides, not the driver

Python sits on the typed pyo3 binding and is correct at every depth for
temporals (nanoseconds, zones), paths, points, NaN/Inf and
entity-vs-map discrimination. Java and JS sit on the JSON C ABI and share the
same gaps row for row:

| Type | Java / JS (JSON ABI) | Python (typed binding) |
| --- | --- | --- |
| Temporals nested in list/map/**entity property** | broken (STRING) | correct |
| Duration (months / sub-µs) | top-level only | lossy through `timedelta`; nested = string |
| NaN / ±Inf | top level only | correct at every depth |
| PATH nested | LIST | correct |
| User map shaped like an entity | mis-hydrated as NODE | stays a map |
| Element ids | synthesised (hack) | disagree with `elementId()` (hack) |
| VECTOR | dict / map | dict below top level |

**Conclusion: fix conversions in the C ABI, not per client.** A Java-side fix
would be a second guessing layer; JS would need a third. Python's remaining
value gaps (durations, vectors, element ids) are binding-level, not ABI-level.

## 4. Error codes decide retry correctness (measured, twice)

Without a Neo4j status code crossing the boundary, `execute_write` does
**not** retry a write-write conflict and the user's write is **silently lost**
(Python and JS both measured this: final value 100/10 vs the correct 101/11).
Each spike carries a second, lossy copy of the Bolt surface's
`failures::error_class` table (message-sniffing in Java/JS, a kind→code map in
Python) to make retries work. The engine already computes the right answer —
single-sourcing it into the engine and exposing it on the ABI removes every
copy.

## 5. Consolidated requirements for the typed-value + error-code C ABI ADR

Hit in all three legs unless noted:

1. **Error classification, single-sourced in the engine** — Neo4j status code,
   GQLSTATUS, compile-vs-runtime class (ADR-0319), retryable flag. Today three
   tables exist in the repo (bolt `failures.rs`, `binding_error_metadata`, CLI
   `classify`) and drift (the correctness-harness waves keep editing them).
2. **Commit sequence → real bookmarks** (ADR-0310's `relate:<n>`; the
   engine drops it today).
3. **Typed result values at every depth** — opt-in tagged format beside
   `relate_value_json`: temporals with zone/offset, durations as
   months/days/seconds/nanos, paths, entities with element ids (and rel
   endpoints), vectors, non-finite floats, an encoding user maps cannot
   collide with. The JSON form stays for existing callers.
4. **Typed parameters** — the same format inbound, plus (Java) `byte[]`;
   (Python) named-zone datetimes and sub-µs temporals need a carrier.
5. **JS-specific: errors returned with the call** — koffi's async form loses
   `relate_last_error` (thread-local, measured `null`); the client must
   not block the event loop, so failures need an out-param or return shape.
   Related client finding: bare `JSON.parse` rounds int64 beyond 2^53 and
   cannot send float-typed `1` — the JS client needs a lossless number codec
   regardless of the ABI work.
6. **Streaming (cursor ABI)** — deferred for v1; all three legs document
   "results are materialised; `fetchSize` pages a buffer" (the HTTP
   transport's own documented caveat).

Python-only (binding, not ABI): multiple connections per store (the spike's
`_driver_session` seam); EXPLAIN `columns == []`.

## 6. Packaging recommendations (per leg, from the reports)

- **Python:** inside the existing `relate-db` wheel as subpackage
  `relate.neo4j`, behind a `[neo4j]` extra pinned `>=6.4,<6.5`, with an
  import-time version guard. Keeps the private native seam inside one wheel.
- **Java:** a separate `relate-driver` pure-Java artifact depending on the
  fat jar (spike recommendation (b)) — decouples engine-client releases from
  the unsupported bolt-connection SPI's churn.
- **JS:** inside `@relate/relate` as a `./neo4j` subpath export, with
  `neo4j-driver` as an optional peer dependency pinned `~6.3.0` and a
  load-time version check.

All three legs agree on the sequencing: **do not ship v1 with the hacks**
(element-id re-spelling, column-stamp re-typing, message-sniffing error
classification, provider-minted bookmarks) — land the ABI/binding work first.

## 7. Open engine/client bugs found by the spikes

- JS client (main): `JSON.parse` number codec — `RETURN 9007199254740993`
  comes back wrong; a float-typed `1` cannot be sent (pre-existing, needs a
  ticket).
- Java client: no `Database.close()`/`Connection.close()` and no
  parameterised query on the public API (the `DriverSeam` gaps); the two
  Jackson decode bugs were already fixed in #658/LABS-836.
- File-store lock contention surfaces as a 35–65 s retry loop under the
  driver's `ServiceUnavailable` handling (JS leg, measured).
