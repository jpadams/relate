# 0022. Checkpoint shards are byte-budgeted id windows (copy-on-write)

- **Status**: Accepted
- **Date**: 2026-08-30
- **Deciders**: MemoryDB maintainers

## Context

ADR-0005 / ADR-0012 write one `nodes.parquet` and one `relationships.parquet`
per checkpoint. That is a full-graph `put` on `s3://` (and a full local
rewrite). Graphs that still fit in RAM (ADR-0004) already make that too
large: a 256 MiB+ snapshot on every WAL rotate is wasted transfer when
only a few id ranges changed.

Paging the live graph is a separate decision (out-of-core execution).
This ADR only changes the **durable snapshot layout** so a checkpoint
uploads dirty slices.

Row size is not constant. A node with a large property map can be tens of
KiB; a label-only node is hundreds of bytes. A fixed “N ids per file”
therefore cannot keep files near a byte cap.

## Decision

1. **Layout.** `manifest.json` format_version **3** lists shard parts.
   Each part is a Parquet file of live rows (same columns as ADR-0005)
   for an half-open id range `[id_lo, id_hi)`:

   ```
   nodes/{id_lo}-{id_hi}.parquet
   relationships/{id_lo}-{id_hi}.parquet
   ```

   Load reads every part, then WAL replay (`lsn > checkpoint_lsn`) as
   today. format_version 1–2 still load the monolith pair.

2. **Copy-on-write.** The graph records dirty node/rel ids on committed
   mutations. A checkpoint rewrites **only** parts that overlap dirty
   ids (or new ranges). Unchanged objects are not written. Manifest is
   published last. Retired part paths are deleted after the new
   manifest is durable.

3. **Byte budget, derived width.** The cap is **bytes**, default 256 MiB
   (`MEMORYDB_SHARD_TARGET_BYTES`). Id-window width is

   `W = clamp(target / max(ewma_bytes_per_row, 256), 4096, 1_000_000)`

   `ewma_bytes_per_row` is updated from written parts (file bytes /
   row count), separately for nodes and relationships. Until the first
   measurement, assume 1024 bytes/row (conservative: fewer ids per
   window). New high-id windows use the current `W`, aligned to `W`.

4. **Split on overflow.** If encoding a dirty window exceeds the target
   and the window has more than one row, split at the median live id,
   recurse, replace one manifest entry with two. A single row larger
   than the target is allowed (hard exception). Do not globally rehash
   or move ids between existing windows except this split (merge of
   undersized neighbors is follow-on).

5. **Not in this ADR.** Community / graph-locality placement. Out-of-core
   page cache. Delta/tombstone files.

## Alternatives considered

- **Fixed id count per shard** — simple, file size tracks property
  fatness. Rejected; the S3 cost is bytes.
- **Hash(id) % N** — spreads creates; a mixed write load dirties most
  shards. Rejected while ids are sequential and creates dominate.
- **Graph-aware (community) shards** — helps later paging, needs a
  placement map and moves. Rejected for checkpoint-only CoW.
- **Merge-on-read deltas + tombstones** — smaller writes, extra merge
  on every open. Rejected; we take the hit on write (full live image
  of a dirty window).
- **Keep two monolith files** — rejected; S3 full rewrite does not
  scale.

## Consequences

- Open of v3 snapshots lists parts from the manifest; missing a listed
  object is a storage error.
- First checkpoint after a v1/v2 snapshot rewrites into shards and
  drops the monolith names.
- Auto-checkpoint and explicit `checkpoint()` share this dump.
- Execution remains an in-memory LPG (ADR-0004). Shard files are not
  queried in place.

## Later references

ADR-0044 withdraws engine `s3://`. Copy-on-write shards remain the
local Parquet layout; the S3 full-rewrite cost in Context is historical.

ADR-0086 proposes finite persisted-input limits: the single-row over-target
exception and configurable shard target remain, but only within the larger
hard Parquet page, row, decoded-byte, and 1 GiB per-shard recovery envelope.
An unsplittable shard beyond that envelope fails checkpoint without publishing
a new manifest or recycling its WAL.

ADR-0089 proposes superseding the fixed `nodes/{lo}-{hi}.parquet` and
`relationships/{lo}-{hi}.parquet` names for new writes with immutable,
generation-unique paths. Existing v3 fixed-name parts remain readable and
reusable; the manifest is still published last, with old referenced parts
left intact until its durable publication.

ADR-0095 adds optional typed schema theater metadata to the v3 manifest so
recycling WAL after a checkpoint cannot erase enforced-label constraints.

ADR-0120 adds an optional `ontology_path` to the manifest, selecting one
immutable ontology object. A manifest that carries it has
`format_version: 4`; manifests without it stay at v3.

ADR-0183 proposes the merge of undersized neighbors that Decision 4 left as
follow-on: a checkpoint rewrite absorbs adjacent parts while their live
decoded bytes fit the target and writes them as byte-budgeted shards, so ids
also move between neighboring windows on merge. The manifest format is
unchanged.
