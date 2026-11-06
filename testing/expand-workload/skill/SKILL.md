---
name: neo4j-query-workload-skill
description: Grows a graph dataset's query workload the way an application accumulates requirements — a validated base set of real queries, then one new capability per round, each round extending the proven queries and validated against the dataset. Use when generating test-query sets for a graph dataset, probing a Cypher runtime for correctness defects or slow paths, or building a performance suite that reads like the app's feature history. Annotations from each generation source (expected errors, slow budgets) make the validation report a mismatch list, not a red wall. Does NOT write or optimize individual queries — use neo4j-cypher-skill / neo4j-query-tuning-skill. Does NOT cover schema design or data import — use neo4j-modeling-skill / neo4j-import-skill. Does NOT cover GDS workloads — use neo4j-gds-skill.
compatibility: Cypher 25 surface; validation needs the relate Python wheel/CLI or the neo4j Python driver, matching the dataset under test
version: 1.1.0
---

## When to Use

- Growing a test-query workload for a graph dataset in reviewable increments
- Probing a runtime for defects or slow paths with real data, round by round
- Building a perf suite that reads like the application's feature history

## When NOT to Use

- **One query** → `neo4j-cypher-skill` / `neo4j-query-tuning-skill`
- **Schema/import** → `neo4j-modeling-skill` / `neo4j-import-skill`
- **GDS** → `neo4j-gds-skill`

## The core rule

**The workload lives on the dataset's model.** Every query references only labels,
relationship types, and properties present in the schema input. Nothing foreign
enters: no TCK fixtures, no demo shapes, no invented names. Corpus material (TCK,
cheat sheet) is shape *inspiration* for the agent — consulted per round, rewritten
onto real names, never bulk-imported. The TCK shelf is the spec-harness checkout
(`features/tck/`, `features/relate-tck/`, `features/cip-eval/`) or any openCypher
TCK checkout — `references/sources.md` names the paths.

## Inputs

| Input | Sources |
|---|---|
| Schema | `<db>-schema.json` if present; else introspect live (`CALL db.schema.visualization`, label/type/property listing + per-label counts) |
| Seed queries | The app's real queries: docs, blog posts, repo examples, the user's input |
| Dataset access | relate store path or `:memory:`; or `NEO4J_URI`/`NEO4J_USERNAME`/`NEO4J_PASSWORD` |
| Round budget | How many rounds (default: until the coverage backlog is empty or declared out-of-scope) |

No seeds → stop. A workload grown from nothing is synthetic noise.

## The loop

### Round 0 — base

The seed queries, as-is. Validate. This is the application's v1: if a seed already
fails, fix the dataset understanding first — everything grows from a green base.

### Round N — one new requirement

1. **Pick the next capability** from the coverage backlog
   (`references/coverage-map.md` — ordered by dependency: filtering before
   aggregation, piping before subqueries, paths before quantified patterns, writes
   last). Skip a capability only with a recorded reason (missing data type, engine
   gap).
2. **Grow it like a feature request** (`references/generation-recipes.md`):
   - Extend existing workload queries with the capability — the tag listing gains
     paging; the unanswered query gains an aggregation; the engagement read gains a
     subquery. Growth rides on queries already proven in earlier rounds.
   - Add the few base queries that introduce the capability where nothing extends.
   - Let the dataset grow too when the requirement implies it (a synthetic
     `__wl_`-keyed entity per write round, deleted by the same or the next entry).
3. **Validate as you generate**: every new entry runs against the dataset the
   moment it is written — a query that was never executed must never reach the
   workload. Fix or drop failures immediately; the round-end batch validation
   is the regression check, not the first time anything runs.
4. **Annotate expectations at the source**: `expect:"error"` (+kind/detail) for
   deliberate negatives, `expect_ms` for heavy-but-legitimate shapes,
   `expect_slow` for known-slow ones you keep on purpose.
5. **Validate the whole workload** (`references/validation.md`):

```
python3 scripts/run_workload.py workload.jsonl --engine relate --store <path> --out report.json
```

6. **Diff round-over-round**: the report shows what round N found (new statuses vs
   round N−1) and that N−1's entries are unchanged. A previously-green entry turning
   red is a regression finding, the round's headline.

### Stop

When the backlog is empty, or every remaining capability has a recorded reason.
Report the covered matrix, the defect list, and the per-tier/per-round perf table.

## Shape rules — every entry

- `extends` names the entry it grew from (empty for base introductions); the growth
  chain is the artifact's skeleton.
- One statement per entry; parameters for every data literal.
- Writes are idempotent or self-cleaning on `__wl_` synthetic keys.
- Tier is structural and checked against `references/generation-recipes.md` — a
  fixed-length hop count is never "complex".
- Predicates must be able to match data (use the schema's counts); an all-empty
  workload proves nothing.

## Deliverable

`workload.adoc` — readable like a feature history, runnable like the dataset guides:
`== Round 0 — the base queries`, `== Round 1 — paging arrives`, … each entry with its
provenance note (seed / growth operator / capability / TCK-inspiration) and its
validated outcome. `workload_to_adoc.py` renders it from the validated report; the
validator reads the adoc back, so the artifact is the workload.
