# Push into Neo4j / Aura

```bash
relate push --to CREDS --database PATH \
  [--query Q | --labels L1,L2 | --types T1,T2] \
  [--batch-size N] [--nodes-update POLICY] [--rels-update POLICY] \
  [--updated-at NAME[,NAME]] [--rel-mode MODE] \
  [--merge-key LABEL:PROP[,PROP]] [--rel-key TYPE:PROP[,PROP]] \
  [--unkeyed-labels ACTION] \
  [--dry-run] [--json]
```

- `--to CREDS` — required: the same env file `pull`'s `--from` reads
  (`NEO4J_URI`, `NEO4J_USERNAME`, `NEO4J_PASSWORD`, optional
  `NEO4J_DATABASE`), or a Query API URI (username/password then from the
  process environment).
- `--database PATH` — required source store (not `:memory:`).
- Scope: `--query Q` (what the statements RETURN — nodes, rels, paths),
  `--labels`, `--types`, or nothing (the whole store). Exactly one.
- `--nodes-update` / `--rels-update` — `add` (default), `replace`, or
  `skip`, global or per-entity (`Movie=replace,Person=skip`).
- `--updated-at name[,name]` — timestamp candidates; a strictly newer
  remote row is skipped (`remote-newer`), equal-or-newer local applies.
- `--rel-mode keyed|structural|create` (default keyed — merge on the
  type's resolved key: a `--rel-key` override, else its relationship
  uniqueness/key constraint, else one rel of a type between two nodes).
- `--rel-key TYPE:PROP[,PROP]` — merge key for one relationship type,
  overriding or supplying its key (repeatable).
- `--dry-run` prints the plan without touching the remote; `--json`
  prints the machine-readable report.

Push is `pull`'s mirror: merge keys come from the store's uniqueness and
node/relationship key constraints (composite keys included), nodes and rels
are grouped by entity and sent as batched parameterized
`UNWIND $rows AS row MERGE (n:`Label` {key: row.key}) SET …` statements,
and every value is a typed parameter, so `DATE`, `POINT`, `UUID` and
vector properties keep their Cypher type on the remote. A label with no
merge key is reported and skipped by default — `--merge-key` supplies a
key, `--unkeyed-labels create` switches to one-shot (non-idempotent)
`CREATE` rows. Before each write the remote's current keyed rows are
pre-read, so the report says what was created, updated, and skipped (by
reason), and `--updated-at` can compare timestamps.

Idempotent re-push is the contract: the same push twice lands the same
remote state. Nothing is ever deleted remotely (deletion sync is a later
design, ADR-0401 §7). A failing batch fails the push naming the entity,
batch, and row range, and the report lists exactly which batches landed
(exit 1). Exit codes: 0 success (a dry run included), 1 runtime/remote, 2
usage.

Try it against the hosted demo remote (see [pull](pull.md) for the
credentials) after pulling a subgraph:

```bash
relate push --to demo.env --database ./companies --labels Company --dry-run
```
