# Golden stores

Small stores committed in a released on-disk format, one versioned set per
generating release (ADR-0114 §3). They are the compatibility fixtures for
later format ADRs: a reader must keep opening every set here, and a later
format ADR (e.g. ADR-0120's `MDBWAL02` / manifest v4 / `value_encoding` 3)
adds a new set next to the existing ones. **A set is never regenerated in
place.**

Under ADR-0127's migrate-on-open contract, a fixture whose markers are
readable but no longer current is also a migration fixture: opening a
**copy** of it upgrades the copy to the current format in place. There is
one legacy reader per readable version plus the current writer, and
migration is the single open flow — legacy-reader load, WAL replay,
current-format checkpoint, WAL rotation — on the handles already open; the
migration report is internal (`pub(crate)`) and no log line is emitted.
The golden tests always copy a fixture into a temporary directory before
opening, so the committed bytes here stay at their generating release's
format. Both committed sets are already at the current format, so opening
either one performs no migration.

## v0.8.0/

Written by CypherLite `v0.8.0`
(commit `2397a5daf0e06ffe1dc61d9d18e5d9eef3221980`). Every store holds the
same graph and catalog: a checkpoint, then a WAL tail (the generator
dropped the database without a final checkpoint).

- `parquet/` — Native directory, Parquet snapshot codec.
  Format markers: `manifest.json` `"format_version": 3`,
  `wal/*.log` segments starting with `MDBWAL01`, `.cypherlite.lock`
  (root role) and `wal/.cypherlite.lock` (wal role) marker files.
- `json/` — Native directory, JSON snapshot codec.
  Format markers: `graph.json` `"value_encoding": 2`, `manifest.json`
  `"codec": "json"`, `MDBWAL01` WAL segments, the same two lock markers.
- `graph.cl` — single-file bundle (ADR-0074), Parquet snapshot codec.
  Format markers: virtual `manifest.json` `"format_version": 3` and
  `MDBWAL01` WAL segments inside the bundle; the host file starts with
  `MDBCLB01`.

Content (asserted by `relate/relate/tests/golden_stores.rs`):

- Schema: range index `person_name`, vector index `person_embedding`
  (3 dimensions, cosine), fulltext index `person_bio`, uniqueness
  constraint `person_email_unique`, existence constraint
  `person_name_exists` — all on `:Person`.
- Databases: `analytics` (created) and `staging` (WAL tail only);
  `neo4j`'s default language altered to `CYPHER 5`.
- Graph: `:Person:Scientist` node `Ada` and `:Person` node `Charles`,
  joined by a `KNOWS` relationship with properties. Node properties cover
  string, integer, float, boolean, list, date, datetime with offset,
  duration, localtime, point, and vector values.
- WAL tail (records above the snapshot's `checkpoint_lsn`): `CreateNode`,
  `CreateRel`, `SetNodeProp`, `RemoveNodeProp`, `SetRelProp`, `AddLabels`,
  `RemoveLabels`, `DeleteNode`, `DeleteRel`, `PutDatabase`, plus
  `CreateIndex`/`DropIndex` for a scratch index.

### Regenerating (only for a new set, never in place)

The generator is the ignored `write_golden_stores` test in
`relate/relate/tests/golden_stores.rs`. It runs against a detached
worktree at the generating tag so concurrent edits in the main tree cannot
leak into the fixtures:

```sh
git worktree add --detach /tmp/relate-v0.8.0 v0.8.0
cp relate/relate/tests/golden_stores.rs \
  /tmp/relate-v0.8.0/relate/relate/tests/
cd /tmp/relate-v0.8.0
RELATE_GOLDEN_STORES_OUT=<repo>/testdata/stores/v0.8.0 \
  cargo test -p relate --test golden_stores write_golden_stores \
  -- --ignored --exact --nocapture
cd <repo> && git worktree remove /tmp/relate-v0.8.0
```

The generator panics when `RELATE_GOLDEN_STORES_OUT` is unset or names
a non-empty directory.

## v0.16.0/

Written by CypherLite `v0.16.0`
(commit `ca84dd937eabe18ac84be5e9578f53c4c9cbac69`). Every store holds the
same schema, graph and catalog as the v0.8.0 set, written by a current
binary: a checkpoint, then a WAL tail (the generator dropped the database
without a final checkpoint).

- `parquet/` — Native directory, Parquet snapshot codec.
  Format markers: `manifest.json` `"format_version": 3`,
  `wal/*.log` segments starting with `MDBWAL01`, `.cypherlite.lock`
  (root role) and `wal/.cypherlite.lock` (wal role) marker files. Unlike
  the v0.8.0 set it also carries the current store layout: a
  `.cypherlite.writer` lease beside each lock marker (ADR-0136) and the
  `.cypherlite/schema/schema.cypher` mirror (ADR-0481).
- `json/` — Native directory, JSON snapshot codec.
  Format markers: `graph.json` `"value_encoding": 2`, `manifest.json`
  `"codec": "json"`, `MDBWAL01` WAL segments, the same lock, lease and
  schema-mirror files.
- `graph.cl` — single-file bundle (ADR-0074), Parquet snapshot codec.
  Format markers: virtual `manifest.json` `"format_version": 3` and
  `MDBWAL01` WAL segments inside the bundle; the host file starts with
  `MDBCLB01` (superblock `VERSION` 1).

Content (asserted by `relate/relate/tests/golden_stores.rs`, which
opens every committed set):

- Schema: the v0.8.0 set's five objects — range index `person_name`, vector
  index `person_embedding` (3 dimensions, cosine), fulltext index
  `person_bio`, uniqueness constraint `person_email_unique`, existence
  constraint `person_name_exists` — all on `:Person`.
- Databases: exactly the three fixed namespaces (ADR-0116 §1), with
  `neo4j`'s default language altered to `CYPHER 5` by the WAL tail's
  `PutDatabase` row. A v0.16.0 binary rejects `CREATE DATABASE` (51N27),
  so this set carries no theater rows.
- Graph: `:Person:Scientist` node `Ada` and `:Person` node `Charles`,
  joined by a `KNOWS` relationship with properties; the node properties
  cover the same kinds as v0.8.0 (string, integer, float, boolean, list,
  date, datetime with offset, duration, localtime, point, vector).
- WAL tail (12 records above the snapshot's `checkpoint_lsn` 8):
  `CreateNode`, `CreateRel`, `SetNodeProp`, `RemoveNodeProp`, `SetRelProp`,
  `AddLabels`, `RemoveLabels`, `DeleteNode`, `DeleteRel`, `PutDatabase`,
  plus `CreateIndex`/`DropIndex` and their `CatalogIndexDetail` for a
  scratch index.

### Regenerating (only for a new set, never in place)

Same recipe as v0.8.0 above, at tag `v0.16.0`, with the generator's
statement lists adjusted to what that tag accepts: the two
`CREATE DATABASE` rows leave `PRE_CHECKPOINT` and `WAL_TAIL` (ADR-0116 §1
turns them into 51N27 errors), and `ALTER DATABASE neo4j SET DEFAULT
LANGUAGE CYPHER 5` moves from `PRE_CHECKPOINT` into the tail, which keeps
`PutDatabase` tail coverage. The checkpoint then lands at `checkpoint_lsn`
8 and the restored graph, catalog and database rows match the v0.8.0 set.
