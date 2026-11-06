# Validation protocol

Every entry executes against the dataset before it counts. Mechanical, scripted,
repeatable — and **cumulative**: a round's report must show the previous rounds
unchanged, so a round is a regression test for the runtime too.

## Entry format (`workload.jsonl` / the adoc's blocks)

```json
{"id": "r3.unanswered-ext.002", "round": 3, "area": "subqueries", "tier": "mid",
 "kind": "read", "query": "MATCH (q:Question) WHERE … RETURN …", "params": {"min": 5},
 "extends": "r0.unanswered.001", "source": "growth:subquery",
 "expect": "ok"}
```

- `round`: which round introduced the entry (0 = base).
- `extends`: the parent entry id it grew from (absent for base introductions).
- `source`: provenance — `seed:<origin>`, `mutation:<op>:<base>`,
  `growth:<capability>` (the parent is `extends`, not repeated here),
  `tck:<feature>` or `composed`.
- `expect` (optional): `ok` (default) | `error` — negatives carry `expect_kind` /
  `expect_detail` / `expect_phase`.
- `expect_ms` (optional): soft timing budget; over it marks the entry `slow`.
- `expect_slow` (optional): tolerate an outright timeout; recorded, not flagged.
- `skip_reason` (optional): never run, recorded.

In the adoc these serialize as block attributes:
`[source,cypher,round="3",extends="r0.unanswered.001",expect="error:VariableAlreadyBound",expect_ms="500"]`.

## Running

```
python3 scripts/run_workload.py workload.jsonl --engine relate --store <path> --out report.json
python3 scripts/run_workload.py workload.adoc    --engine neo4j     --out report.json
```

Engines: relate (Python wheel, else the CLI per statement) or Neo4j (driver from
`NEO4J_URI` / `NEO4J_USERNAME` / `NEO4J_PASSWORD`). Reads run first, in round order;
writes run last, isolated: on a clone of the file store (relate), a fresh
in-memory rebuild of the setup (`:memory:`), or a rolled-back transaction (neo4j).

## Classification (expectations decide what a result means)

| Status | Meaning | Defect? |
|---|---|---|
| `ok` | ran as expected | no |
| `expected-error` | rejected, and the engine's message carries `expect_detail` | no |
| `engine-error` | rejected though expected to succeed | yes |
| `wrong-error` | rejected but with the wrong detail | yes (spec fidelity) |
| `unexpected-ok` | ran though annotated to fail — the engine accepts what must fail | yes (sharpest) |
| `timeout` | over the soft budget without `expect_slow` | yes (slow path) |
| `skipped` | only with `skip_reason`; without one the report fails | no |
| `slow` marker | `ok` but over `expect_ms` | finding, not failure |

## The round report

`report.json` groups by round:

```json
{
  "rounds": {
    "0": {"ok": 8, "median_ms": 0.4},
    "3": {"ok": 12, "expected-error": 1, "median_ms": 1.2}
  },
  "regressions": [
    {"id": "r0.top-tags.001", "round": 0, "was": "ok", "now": "engine-error"}
  ],
  "new_this_round": [...],
  "defects": [...],
  "perf": {"per_tier": {...}, "per_round": {...}, "slowest": [...]}
}
```

- `regressions` — entries whose status changed vs the previous report on the same
  store. The runtime's behavior changed; investigate before the round lands.
- `new_this_round` — entries first seen in the latest round.
- The summary prints rounds in order; a regression is the round's headline.

## Dataset integrity

- After the write entries, the validator re-counts the *original* store (opened
  read-only) — counts must equal the base probe. A leak fails the run (exit 1).
- Every status must be one of the classified set; anything else fails the run.
- A `skipped` entry without `skip_reason` fails the run.

## Perf capture

- Median of `--repeat N` (default 3) per read; writes run once (not repeated —
  they are idempotent probes, not benchmarks; the report marks them).
- Per-tier and per-round: median, p95, max. The 10 slowest overall, with rounds.
- Same store, machine, process — never compare timings across engines without
  saying so.

## Known limitation

Execution is in-process: the soft timeout measures, it does not interrupt. A
pathological query can stall or OOM the validator process (observed with
cross-product CREATE / full-graph DETACH DELETE; see LABS-263/264). The anti-pattern
guards keep known-explosive shapes out; per-write subprocess isolation is the
follow-up. CLI engine mode does kill on timeout (process per statement), at the cost
of startup overhead per query.
