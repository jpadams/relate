# Growth recipes

How a capability becomes queries. The unit of work is the **round**: one capability,
grown onto the existing workload the way a feature request extends an application.

## The two moves per round

**1. Extend** — take queries already proven in earlier rounds and grow the new
capability onto them (`extends` names the parent). Examples:

- paging arrives → the top-tags listing gains `ORDER BY questions DESC SKIP 10
  LIMIT 10`; the unanswered listing gains a `SKIP/LIMIT` pair too.
- aggregation arrives → the "users who answer" read gains `WITH u, count(*) AS n
  WHERE n > 1 RETURN …`; the tag listing gains `collect(q.title)[..3]`.
- pattern predicates arrive → "unanswered" becomes "unanswered *or answered only by
  someone who never asked*": `WHERE NOT (q)<-[:ANSWERED]-() OR NOT EXISTS { (q)<-[:ASKED]-() }`.
- subqueries arrive → the top-tags listing becomes "tags whose questions average
  above the site average": `WHERE COUNT { (q)-[:TAGGED]->(t) AND q.score > $avg } > 0`.
- paths arrive → the user-engagement read extends to 2–3 bounded hops over the
  densest rel type.

**2. Introduce** — a few base queries that introduce the capability where nothing
extends cleanly (the first aggregation, the first OPTIONAL MATCH). Introductions are
the exception, not the mode: if most of a round is introductions, the capability was
picked too early (its prerequisites are unproven).

## Entry points: derive the anchor, then expand

Real workloads anchor on a selective entry point and walk out — and the anchor
comes from the data, never from a hardcoded literal:

- **Derive it in the same statement**: `MATCH (u:User) WITH u.display_name AS
  anchor ORDER BY … LIMIT 1 MATCH (a:User {display_name: anchor})-[:ASKED]->…`
  — the anchor query runs first, the anchored walk follows. Any dataset, no
  input data, always hits something real.
- **Anchor sets, not just one**: pick N per the query shape (`LIMIT 10` top
  answerers, then `WHERE u.display_name IN anchors` before the expand) — a
  multi-anchor probe exercises the runtime the way a dashboard does.
- **Derive from the graph's own structure**: the best anchor is often
  computed, not looked up — the node with the most outgoing edges
  (`COUNT { (n)-[:T]->() }`), the endpoints of an existing chain
  (`nodes(p)[0]` / `nodes(p)[-1]`), the label with the highest count. For a
  shortest-path round, derive both ends from one real chain so the path is
  guaranteed to exist.
- Anchors from prior rounds' outputs (the round-0 top tag feeds the round-3
  traversal) tie the growth chain to the data, not to constants.

## Shape rules

- Every name comes from the schema: labels, rel types, property keys, indexes.
  Nothing foreign enters — TCK patterns and cheat-sheet examples are shape
  inspiration the agent rewrites onto real names *and keeps only when the rewrite
  preserves the point*; a pattern that cannot be rewritten without leaving the
  dataset's model is dropped, not diluted.
- Predicates must be able to match: use per-label counts and sampled values from the
  schema; `WHERE n.x = $v` where `$v` was seen in a sample; ranges on numerics;
  `IS NOT NULL` on sparse properties. Allow empty results only when the round's
  point *is* null/empty semantics.
- Parameters for every data literal (`$name`), values recorded in `params`.
- One statement per entry. One growth per extension — a query that adds two new
  capabilities belongs to two different rounds; pick one.

## Expectation annotations (decided at generation)

- `expect:"error"` + `expect_kind`/`expect_detail` — deliberate negatives: probe an
  engine rule on purpose (only when the round's capability motivates it — e.g. the
  MERGE round probing null-key rejection). Never mutate a query into a negative by
  accident.
- `expect_ms` — heavy-but-legitimate shapes (path counting, big aggregations);
  over budget marks the entry slow, not failed.
- `expect_slow` — known-slow shapes kept on purpose to document them; the validator
  records, not flags.
- `skip_reason` — never run, recorded. For anti-pattern shapes kept as documentation.

## Mutation operators (micro toolset inside a round)

Per parent entry, at most one operator per variant; operators serve the round's
capability, they are not a bulk expansion pass:

1. `dir-flip` — reverse one relationship direction
2. `opt-wrap` — MATCH → OPTIONAL MATCH with a null-safe projection
3. `distinct` — toggle DISTINCT (RETURN and count())
4. `agg-swap` — count→collect, avg→sum, min→max, same grouping
5. `limit-page` — LIMIT n → SKIP k LIMIT k
6. `param-lift` — literal → `$param` (value recorded)
7. `with-split` — split the projection across a WITH boundary
8. `order-expr` — ORDER BY a non-returned expression
9. `label-add` / `label-drop` — one label condition on an existing pattern node

`scripts/mutate_queries.py` applies them mechanically when the round calls for
variants; judgment decides which mutations say anything.

## Anti-patterns (reject on sight)

- A name not in the schema
- Unbounded variable-length paths without a selective anchor
- Unanchored cross products (`MATCH (a), (b), (c)`)
- Writes touching anything without a `__wl_` synthetic key
- Tier inflation (a WITH chain is mid; a fixed hop count is never complex)
- Literal-only statements (`RETURN 1+1`, truth tables) — they test the parser, not
  the dataset; a round may include a handful for baseline parser coverage, never more
- Empty-by-construction predicates, except when the round is about null/empty
  semantics and says so
