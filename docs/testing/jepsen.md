# Jepsen certification

The Jepsen harness (`testing/jepsen/`, LABS-649..657) crashes, pauses, and
power-cuts a running `relate serve` while concurrent clients write
through its network surfaces, then checks what the clients were told
against what the store holds. This page is the certification record: what
the harness is, how to run it, the verdict the committed runs support, how
far that verdict reaches, and the running log where new findings go.
Harness internals and every knob are in
[`testing/jepsen/README.md`](../../testing/jepsen/README.md).

## The verdict

The release-gate wording, as the research report fixes it:

> *"under SIGKILL during commit and WAL replay, and under simulated power
> loss (LazyFS), acknowledged writes survived, no Elle anomaly from the
> curated set appeared, and recovery never served a trimmed or zeroed WAL
> — for the tested workloads, fault schedules, and durations."*

Phase 1 alone (LABS-649..652: the driver smoke, the kill -9 set lane, the
widened nemeses, the Elle lane) supports everything up to "and under
simulated power loss". Phase 2 (LABS-653 durability mode through serve,
ADR-0470; LABS-654 the LazyFS lane) adds the power-loss clause. Both
phases are green on the evidence below, so the full sentence is the claim
— and nothing beyond it. The claim is never "Relate is crash-safe" or
"Relate is correct": Jepsen proves the presence of bugs, never their
absence.

Every clause is a per-workload, per-fault statement with a run behind it:

| Clause | Workload × fault × mode × surface | Evidence | Numbers |
| --- | --- | --- | --- |
| acked writes survived SIGKILL + WAL replay | set × kill9 × full × bolt | [`first-green.log`](../../testing/jepsen/runs/first-green.log), [`second-consecutive-green.log`](../../testing/jepsen/runs/second-consecutive-green.log), [`lane-4-full.log`](../../testing/jepsen/runs/lane-4-full.log), [`bolt-post-refactor-green.log`](../../testing/jepsen/runs/bolt-post-refactor-green.log) | 70 s, 5 clients, 4 verified kills per run; acked/recovered 207/207, 204/204, 217/217, 222 acked all recovered; 0 lost, 0 ghosts |
| same, second surface | set + register × kill9 × full × http | [`http-first-green.log`](../../testing/jepsen/runs/http-first-green.log), [`http-second-consecutive-green.log`](../../testing/jepsen/runs/http-second-consecutive-green.log) | set 112/112 and 93/93 acked/recovered; register final value a plausible write (178, 167); 4 kills each |
| same, under freeze/thaw | set × pause × full × bolt | [`nemesis-pause-green.log`](../../testing/jepsen/runs/nemesis-pause-green.log) | 70 s, 232/232; 4 storage-lock rejections of a second serve against the frozen store (flock pin) |
| same, clean-to-crash | set × sigterm-kill9 × full × bolt | [`nemesis-sigterm-kill9-green.log.gz`](../../testing/jepsen/runs/nemesis-sigterm-kill9-green.log.gz) | 110 s, 327/327, 12 verified fault ops |
| same, kills inside the checkpoint window | set × mid-checkpoint × full × bolt | [`nemesis-mid-checkpoint-green.log.gz`](../../testing/jepsen/runs/nemesis-mid-checkpoint-green.log.gz) | 110 s, 437/437; `checkpoint_lsn` advanced between every restart and the next kill (5 of 5 pairs, e.g. 42→104, 333→421) |
| relaxed modes stay inside their loss bound | set × kill9 × everysecond / snapshot (2 s) × bolt | [`lane-2-everysecond.log`](../../testing/jepsen/runs/lane-2-everysecond.log), [`lane-3-snapshot.log`](../../testing/jepsen/runs/lane-3-snapshot.log) | 192/192 and 189/189; 0 lost outside the 1 s / 2 s window |
| acked writes survived simulated power loss | set × poweroff (LazyFS) × full × bolt | [`poweroff-green.log.gz`](../../testing/jepsen/runs/poweroff-green.log.gz) | 5 consecutive hermetic 70 s runs; 1,253 acked writes (260, 269, 260, 207, 257), all recovered; 19 power-offs, each with its clear-cache fault delivered and verified |
| no Elle anomaly from the curated set | counter (list-append) and bank (rw-register) × none × full × bolt | [`elle-counter-green.log.gz`](../../testing/jepsen/runs/elle-counter-green.log.gz), [`elle-bank-green.log.gz`](../../testing/jepsen/runs/elle-bank-green.log.gz) | 330 s, 5 clients each; counter 3,272 txns (1,996 ok, 1,276 fail, 0 info); bank 3,203 transfers (2,655 ok, 548 fail) + final read, bank checker valid (total 1,600 conserved); SI and serializable gates `:valid? true` |
| same, across crashes | counter × kill9 × full × bolt | [`elle-counter-kill9.log.gz`](../../testing/jepsen/runs/elle-counter-kill9.log.gz) | 90 s, 535 txns (158 ok, 123 fail, 254 info), 5 kills fired, 5 restarts recovered; gates still `:valid? true` |
| recovery never served a trimmed or zeroed WAL | every crash and power-off row above | the final-state checker of each run | a recovery that trimmed or zeroed acked WAL records surfaces as lost acked writes; every row reports 0 |

Each verdict reads as its own narrow sentence. For example: *no
acknowledged-write loss and no fabricated rows across 5 consecutive 70 s
LazyFS power-off runs (19 power-offs, 1,253 acked writes) in `full` mode
over Bolt* — not "Relate survives power loss".

### Calibration: the faults can fail

A green lane certifies only if the same fault fails a known-bad
configuration (the Jepsen Lite principle). Both fault families are
calibrated on record:

- **kill -9**: the Memory VFS negative control
  ([`lane-1-memory-vfs-control.log`](../../testing/jepsen/runs/lane-1-memory-vfs-control.log))
  lost **206 of 206** acknowledged writes under the same kill schedule —
  the lane can see loss. `lanes.sh` runs it first; it passes only by
  losing.
- **power-off**: the snapshot-mode control on the same LazyFS rig and
  nemesis
  ([`poweroff-control-lost-acked.log.gz`](../../testing/jepsen/runs/poweroff-control-lost-acked.log.gz))
  lost **275 of 275** acknowledged writes, with 4 power-offs and 4
  clear-cache faults verified. `run-poweroff.sh` fails if the control
  comes back green.

## Findings so far

Every finding the program has produced, most severe first. Engine and
surface defects carry Linear tickets; harness findings were fixed in the
same change that logged them.

| Finding | Class | Ticket | Status |
| --- | --- | --- | --- |
| ORDER BY on a pre-aggregation variable is accepted (Neo4j rejects) | engine, divergence A (4 of 13 cases) | LABS-665 | open, not durability-relevant |
| DISTINCT + ORDER BY loses a MATCH-defined variable (`UndefinedVariable`) | engine, divergence B (5 of 13), genuine scope bug | LABS-666 | open, not durability-relevant |
| Zero-arg `size()` / `toString()` return `null` instead of erroring | engine, divergence C (4 of 13), arity-validation gap | LABS-667 | open, not durability-relevant |
| Driver 5.x identity gate: every current Java driver refuses the default `Relate/…` agent | surface, Blocker for Bolt clients | LABS-649 | fixed: `serve --bolt-identity` (ADR-0293) |
| No Java driver speaks the HTTP Query API | surface fact | LABS-655 | documented: the HTTP lane speaks the wire directly |
| Concurrent lanes shared one compose project — a sibling's wipe destroyed a run's store (202 acked writes lost to the wrong owner) | harness | — | fixed: `JEPSEN_SUFFIX` isolation, verified (not exit-code-trusted) volume wipe |
| jepsen 0.3.12–0.3.14 `grepkill!` made kill nemeses silent no-ops | harness/tooling | — | fixed: jepsen 0.3.15 pin |

Nothing found so far touches durability or isolation: zero acknowledged
writes lost in `full` mode across every fault family, and no curated Elle
anomaly. The divergence classes are dialect-strictness gaps the TCK floor
did not cover.

## What Jepsen can and cannot certify here

Relate is one process with one fsync'd local WAL (ADR-0077), so the
harness runs at n=1. That scope is coherent — concurrent sessions against
one server are all Elle needs, and crash durability is a single-node
property — but it bounds what any run can say.

- **kill -9 cannot validate fsync.** The kernel page cache sits outside
  the killed process, so bytes written but never fsynced still reach the
  file. The kill-lane rows certify the ack boundary and WAL replay, not
  the barrier. That is also why the relaxed lanes observe zero loss under
  kill -9 and assert only the bound.
- **The LazyFS power-off is the only fsync-validating fault.** LazyFS
  (pinned `fa7d32e`) holds un-fsynced bytes in its own memory; the
  power-off drops them (clear-cache) and kills serve and LazyFS together.
  Only that lane distinguishes a store that fsyncs before ack from one
  that does not.
- **The filesystem, not the disk.** LazyFS models lost page cache; drive
  write caches, write reordering, and torn sectors are out of scope.
- **Syscall protocol is invisible.** Whether the WAL writer fsyncs the
  parent directory on rename, or orders checkpoint publication correctly,
  is not visible to a black-box checker unless it happens to lose a
  write. That is ALICE-style crash exploration, not Jepsen.
- **No partitions, membership, or clock faults.** They are meaningless at
  n=1, and containers have no real clocks.
- **Linux fsync only.** The DB node always runs in a Linux container, so
  every verdict is about `fdatasync`; the macOS barrier question
  (ADR-0077) is not exercised.
- **No WAL-corruption nemesis yet.** Bit flips, truncation, and
  misdirected writes against the WAL (`jepsen.nemesis.file`) have not run;
  the "trimmed or zeroed WAL" clause rests on the final-state checker
  under crash and power-off, not on injected corruption. ADR-0114's
  torn-tail rule stays pinned by the storage tests.

### Limits of the Elle verdict

- **SI ≡ serializable by workload shape.** Relate ships snapshot
  isolation; write skew (G2) is a documented non-goal (ADR-0015,
  ADR-0028). The curated set checks `[:snapshot-isolation
  :strong-session-snapshot-isolation]`, which permits G2. The lanes also
  gate on `[:serializable]`, but only because every transaction in both
  workloads writes what it reads (counter reads a key only if it appends
  to it; bank reads and writes both accounts). The serializable verdict
  is a property of these workloads, not a claim that Relate is
  serializable.
- **Strict serializability is discovery-only.** It is recorded
  (`:discovery-verdict true` in both green runs) and never fails a run.
- **Conflict granularity is node identity** (ADR-0015 §4), measured, not
  assumed: two transactions SETting different properties of one node
  conflict, and the second commit fails with
  `Neo.TransientError.Transaction.Outdated` in 5 of 5 iterations, the
  same as the same-property control
  ([`granularity-measurement.log`](../../testing/jepsen/runs/granularity-measurement.log)).
  The serializable argument rests on it.
- **Bolt only.** HTTP is auto-commit per request (ADR-0028), so it cannot
  carry explicit-transaction workloads until LABS-647.
- **Predicate anomalies are out of scope**, as they are for Elle's sound
  core.

## The harness

- **Topology**: Docker Compose with two containers. The control container
  runs JVM 21, the Clojure CLI, and jepsen 0.3.15 (0.3.12..0.3.14's
  `grepkill!` made kill nemeses silent no-ops). The single DB node is the
  **stock product image** that `make docker` builds, so the binary under
  test is the binary that ships. The power-off lane swaps in
  `db-lazyfs.Dockerfile`: the same product binaries on an Ubuntu
  userspace that mounts LazyFS.
- **Client**: the official Java driver (5.28.5, plain Clojure interop)
  over Bolt 4.4. The DB node runs `serve --bolt-identity Neo4j/5.26.0`
  (ADR-0293), because driver 5.x rejects any server agent that doesn't
  start with `Neo4j/` (see the running log). The harness asserts the
  identity on every connection. Only explicit, unmanaged transactions are
  used, because managed `executeWrite` auto-retries the write-write
  conflict code the Elle lane must observe. The HTTP lane speaks the
  Query API v2 wire directly (`java.net.http` + data.json); no Java
  driver version supports it.
- **Ack boundary**: COMMIT SUCCESS on Bolt, the fully consumed HTTP 200 on
  HTTP. A received FAILURE is `:fail`; a connection loss is `:info`.
- **Faults are verified, not trusted**: container state, exit-code
  signature (137 for SIGKILL, 0 plus the shutdown line for SIGTERM), and
  a wire probe that must fail. The fault-evidence checker fails a run
  unless every fault fired and every restart came back.
- **Hermetic runs**: `setup!` kills the DB, wipes the data volume and
  checks that `/data` is empty, asserts the lane's config really reached
  the container (durability mode, store path, checkpoint interval), and
  checks that the image refuses an invalid `RELATE_DURABILITY_MODE`
  (so a stale image can't silently run `full`).

### The matrix

| Axis | Values | Selected by |
| --- | --- | --- |
| Workload | `set` (acked ⊆ recovered ⊆ attempted, no ghosts), `register` (HTTP), `counter` (Elle list-append), `bank` (Elle rw-register + bank checker), `compare` (Neo4j differential) | `WORKLOAD` |
| Fault | `kill9`, `pause` (+ flock pin), `sigterm-kill9`, `mid-checkpoint` (5 s checkpoints, ADR-0020/0022 window), `poweroff` (LazyFS); Elle lanes `none` / `kill` | `NEMESIS`; `run-poweroff.sh` |
| Durability | `full`, `everysecond`, `snapshot`, `memory-vfs` (negative control) — ADR-0470, ADR-0077 | `LANE` |
| Surface | `bolt`, `http` | `run.sh` / `run-http.sh` |

Not every cell has run. The evidence table above lists exactly the cells
with committed runs; any other cell carries no verdict.

## Running it

Prerequisites: Docker with Compose v2, and the product image built from
the repo root:

```sh
make docker
cd testing/jepsen
```

| Script | What it runs | Knobs |
| --- | --- | --- |
| `./run.sh` | one lane: default `set` × `kill9` × `full` over Bolt, 5 clients, 70 s | `WORKLOAD`, `NEMESIS`, `LANE`, `TIME_LIMIT`, `CONCURRENCY`, `NEMESIS_INTERVAL`, `CHECKPOINT_INTERVAL_SECS` (default 300), `RELATE_IMAGE` (default `relate-bolt:latest`), `COUNTERS`, `ACCOUNTS`, `MAX_TRANSFER`, `OPS`, `SEED`, `JEPSEN_SUFFIX` |
| `./lanes.sh` | the ordered durability matrix: memory-vfs control first, then everysecond, snapshot (2 s), full last; aborts on the first red lane | builds and pins `relate-bolt:jepsen-lanes`; sets `JEPSEN_SUFFIX=relaxed-lanes` |
| `./run-http.sh` | `set` + `register` × `kill9` over HTTP Query API v2 | `TIME_LIMIT` (70), `CONCURRENCY` (5), `NEMESIS_INTERVAL` (5) |
| `./run-poweroff.sh` | FUSE guard, host semantics probe, `FULL_RUNS` full-mode power-off runs, then the snapshot control | `FULL_RUNS` (5), `TIME_LIMIT` (70), `CONCURRENCY` (5), `NEMESIS_INTERVAL` (5), `CONTROL_MODE` (snapshot), `CONTROL_CHECKPOINT_INTERVAL_SECS` (3600), `SKIP_PROBE` |

```sh
./run.sh                                   # set × kill9 × full, bolt
NEMESIS=mid-checkpoint ./run.sh            # kills inside the checkpoint window
WORKLOAD=counter ./run.sh                  # Elle counter, 330 s, no faults
./run-poweroff.sh                          # Linux / FUSE-capable host only
```

Re-running specific evidence:

```sh
WORKLOAD=bank ./run.sh                     # the bank lane (Elle rw-register)
NEMESIS=kill WORKLOAD=counter ./run.sh     # the counter lane across crashes
OPS=1000 SEED=657 ./run.sh compare         # the differential sweep, fixed seed
```

`compare` starts a **neo4j:5.26.0** reference container beside the DB node
(compose profile `compare`; first run pulls the image) and divergences land
in `runs/compare-<seed>-divergences.json`. The conflict-granularity
measurement is a standalone script against a running serve:

```sh
docker compose --profile compare up -d db neo4j   # or any serve --bolt
docker compose run --rm --no-deps control \
  clojure -M:granularity bolt://localhost:7687 jepsen jepsen
```

The checker-contract suite (no DB needed — pins the checker semantics
themselves) is the same command CI runs:

```sh
docker compose run --rm --no-deps control clojure -M:test
```

Committed evidence logs over 1,000 lines ride gzipped
(`runs/<name>.log.gz`); read them with `gunzip -c`.

Exit codes are jepsen's: `0` all checks passed (for `memory-vfs`, that
means the control lost acked writes), `1` a check failed, `2` unknown,
`254` bad arguments. `run-poweroff.sh` exits `77` (skipped) where
`/dev/fuse` is absent, so plain macOS hosts skip the power-off lane.

**Isolation (`JEPSEN_SUFFIX`).** Concurrent runs on one docker daemon must
not share a compose project, DB container, or data volume: one run's
hermetic wipe empties the other's store mid-run (observed: a run lost 202
acked writes to a store it no longer owned). `JEPSEN_SUFFIX=<name>
./run.sh` gives the run a private `relate-jepsen-<name>` project,
container, and volume. `run-http.sh` (`relate-jepsen-http-lane`) and
`run-poweroff.sh` (`relate-jepsen-poweroff`) pin their own names. The
m2 dependency cache is shared across the suffixed stock lanes (external
volume); the power-off rig pins its own m2, and a suffixed project builds
its own control image.

**CI.** The Jepsen lane is a dedicated workflow (LABS-734):
`.github/workflows/jepsen.yml`, run on demand — `gh workflow run jepsen.yml
--ref <branch-or-tag>` (a `time_limit` input, default 90, and a `ref` input
for certifying any ref). Main is certified nightly at 04:50 UTC
(LABS-734's cadence; weekly later is a one-line cron change). It builds the
product image, runs the checker-contract suite, then `./run.sh` (set × kill9 × full); any nonzero
verdict fails the run, uploading the `jepsen-run` artifact. The per-PR
matrix does not run Jepsen; drafts (and every PR) keep
`jepsen-harness-sanity` in `ci.yml`, which validates the compose topology.
The other lanes run on demand. Their committed logs under
`testing/jepsen/runs/` are the evidence; timestamped logs are gitignored.

## Running log

This section is the canonical running log of findings from the
certification program. It absorbs the LABS-658 entries, the research
report's post-report findings, and the execution findings since.

**Where future findings go.** Every new finding gets a dated entry here,
in the same change that commits its run log under
`testing/jepsen/runs/`. An entry names the lane, links the log, and
states the minimal reproduction. An engine or surface defect also gets a
Linear ticket in the `relate` project with one Area label and one
Theme label (an engine bug is `engine` or `cypher` + `correctness`), and
the entry cites the ticket id. Harness-only findings (isolation,
tooling) need no ticket unless they leave work behind. A finding that
narrows or widens the verdict must update [The verdict](#the-verdict) in
the same change.

### 2026-11-01 — Java driver identity gate (LABS-649)

Since 5.0, neo4j-java-driver rejects a HELLO SUCCESS whose `server` agent
doesn't start with `Neo4j/` (`UntrustedServerException`), on every
connection, with no config escape hatch. That was verified across 4.4.12,
5.0.0, 5.14.0, 5.26.0, and the 6.x bolt-connection library. The default
`Relate/<version>` identity (ADR-0293) is refused by every current
driver. The remedy is the shipped `--bolt-identity` flag. Behind
`--bolt-identity Neo4j/5.26.0`, the pinned 5.28.5 driver negotiates Bolt
4.4 and the smoke passes: identity, parameters, explicit transactions
with read-your-own-write, and the conflict mapping (the second committer
gets `Neo.TransientError.Transaction.Outdated` as a `TransientException`).
The harness and its DB node run with that flag.

### 2026-11-01 — Native Java driver transport (LABS-656)

In driver 5.28+/6.x, the transport is a ServiceLoader-discoverable SPI
(`org.neo4j.bolt:neo4j-bolt-connection*`, `BoltConnectionProvider`). The
seam is at Bolt messages, not bytes, and the driver's Query API over HTTP
already goes through it. A `relate://` provider over the C ABI is
therefore a moderate adapter project, not a fork. The risks: the SPI is
unsupported and fast-moving, JNI needs large-stack executor threads
(ADR-0259), and auth/bookmarks are no-ops at that layer. This changes
nothing for Jepsen: out-of-process `serve` stays the transport.

### 2026-11-01 — No Java driver speaks the HTTP Query API (LABS-655)

neo4j-java-driver 5.28.5 is Bolt-only: its scheme table lists `bolt`,
`neo4j`, and their TLS variants, and nothing in the `org.neo4j.driver`
group ships an HTTP transport. The HTTP lane therefore uses a raw
`java.net.http` client. It gates identity on the `GET /` discovery
document, which must advertise `/db/{databaseName}/query/v2`.

### 2026-11-01 — Conflict granularity measured (LABS-652)

Node identity, per ADR-0015 §4. Different-property and same-property
concurrent SETs on one node both conflict (5 of 5 iterations each,
[`granularity-measurement.log`](../../testing/jepsen/runs/granularity-measurement.log)).
This was the open question from the research. The Elle serializable
gate depends on it.

### 2026-11-01 — Relaxed modes lose nothing under kill -9 (LABS-653)

As expected: under `docker kill` the host page cache survives, so
`everysecond` and `snapshot` show zero loss. The lanes assert the bound,
never the presence of loss. Observing relaxed-mode loss needs the
power-off fault, and the power-off control confirms it (275 of 275 lost
in `snapshot` mode).

### 2026-11-01 — The power-off control is snapshot mode, not Memory VFS (LABS-654)

A Memory VFS store never touches the LazyFS mount, so it would bypass
the very fault the control must exercise. The honest control is
`--durability-mode snapshot` with a 3600 s checkpoint interval: the same
image, mount, and nemesis, with only the durability policy changed.

### 2026-11-01 — Lane isolation (harness)

Lanes running concurrently on one docker daemon destroyed each other's
DB node mid-run: a sibling's `compose up` recreated the container, and
one run lost 202 acked writes to a store it no longer owned. A
`compose down` also killed another lane's control container. These were
harness defects, not engine defects. The fix is the `JEPSEN_SUFFIX` /
per-script project names above, plus a data-volume wipe that is verified
rather than trusted to its exit code (`docker run -v` auto-creates a
missing volume).

### 2026-11-01 — Neo4j differential: three divergence classes (LABS-657)

The first `compare` run (seed 657, 300 ops;
[`compare-657-found-divergences.log`](../../testing/jepsen/runs/compare-657-found-divergences.log.gz),
[`compare-657-divergences.json`](../../testing/jepsen/runs/compare-657-divergences.json))
ran 311 ops (298 ok, 13 divergences, zero both-error, zero `:info`).
Each divergence shrank to a zero-data minimal case in one of three
classes — 4 in class A, 5 in class B, 4 in class C:

- **LABS-665 — Class A**: Relate accepts ORDER BY on a
  pre-aggregation variable. `MATCH (b) RETURN count(*) AS c ORDER BY
  b.num DESC` returns a row; Neo4j rejects it ("not possible to access
  variables declared before the WITH/RETURN").
- **LABS-666 — Class B, a genuine scope bug**: DISTINCT + ORDER BY loses
  a MATCH-defined variable. `MATCH (b) RETURN DISTINCT b.name AS w ORDER
  BY b.name DESC` fails with `UndefinedVariable: variable 'b' not
  defined`; Neo4j returns rows.
- **LABS-667 — Class C**: zero-argument `size()` / `toString()` return
  `null`; Neo4j errors with "Insufficient parameters". The input is
  degenerate, but this is a real arity-validation gap.

None of these touches durability or isolation, so the verdict above is
unchanged. They are engine findings that the TCK floor did not cover.
