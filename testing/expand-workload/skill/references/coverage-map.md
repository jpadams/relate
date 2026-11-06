# Capability backlog — the Cypher 25 runtime surface, dependency-ordered

Distilled from the Cypher cheat sheet (`neo4j/docs-cypher`, 112 pages; admin pages
excluded: privileges, roles, users, auth, DBMS/server/database management,
constraints, index management, SHOW, transactions, USE/multi-database). This is a
**backlog, not a work plan**: rounds pull one capability per round, in order of
prerequisites, and each capability exists only if the schema can exercise it.

Schema needs vocabulary: `labels:N+` distinct labels, `rels:N+` rel types,
`num-prop`, `str-prop`, `bool-prop`, `list-prop`, `temporal-prop`, `point-prop`,
`text-index`, `vector-index`, `big-graph` (enough nodes for path work), `cycles`.

## Progression

### 1. Read the model (prereq: none)

| Capability | Needs | Grows from | Corpus |
|---|---|---|---|
| single-pattern read + projection | labels:1 | base | clauses/match, return |
| comparison + boolean predicates | num/str-prop | 1 | expressions (comparison, boolean) |
| null semantics (IS NULL / three-valued logic) | sparse prop | 2 | expressions/null |
| ORDER BY + SKIP/LIMIT paging | orderable prop | 1 | clauses/return-orderby, return-skip-limit |
| string predicates (STARTS/ENDS/CONTAINS, =~, concat) | str-prop | 2 | expressions/string |
| introspection (db.labels / db.relationshipTypes / db.schema.visualization) | — | 0 | clauses/call |

### 2. Shape the answer (prereq: 1)

| Capability | Needs | Corpus |
|---|---|---|
| DISTINCT, projection aliasing, `RETURN *` | — | clauses/return |
| aggregation + implicit grouping (count/collect/sum/avg/min/max) | num-prop | expressions/aggregating |
| count(*) vs count(x); collect ordering | — | expressions/aggregating |
| list operators + comprehensions (IN, slicing, [x IN list | …]) | list-prop or collect | expressions/list |
| map projection + dynamic key access (`n[$k]`) | any props | expressions/map |
| CASE bucketing | num/str-prop | expressions/conditional |
| predicate functions over lists (all/any/none/single) | list-prop | expressions/quantifier |
| scalar functions (size, head, last, coalesce, toX) | — | expressions/typeConversion |
| math operators + numeric functions | num-prop | expressions/mathematical |
| UNWIND (stored lists, then collect() results) | list-prop | clauses/* |
| temporal functions/operators | temporal-prop | expressions/temporal |
| spatial functions | point-prop | expressions (point) |
| uuid (randomUUID), database functions (elementId…) | — | — |

### 3. Compose (prereq: 2)

| Capability | Needs | Corpus |
|---|---|---|
| WITH piping + aggregation boundary | — | clauses/* (with) |
| OPTIONAL MATCH + null propagation | rels:1 | clauses/match |
| pattern predicates in WHERE (`WHERE (n)-->(m)`) | rels:1 | expressions/existentialSubqueries |
| expression subqueries (EXISTS/COUNT/COLLECT `{}`) | rels:1 | expressions/existentialSubqueries |
| CALL {} subqueries, correlated | — | clauses/call |
| sequential units (CALL (x) {}) | — | clauses/call |
| UNION / UNION ALL of shaped reads | — | clauses/* |
| label expressions (`:A&B`, `:A|B`) and `%` label predicates (Cypher 25) | labels:2+ | expressions |
| type-predicate expressions (`::INTEGER`) | num/str-prop | expressions |
| LET + FILTER (Cypher 25) | num-prop | — |
| FINISH (Cypher 25) | — | — |
| planner hints (USING INDEX/SCAN/JOIN) — parsed, never plan-changing | index | clauses/match |

### 4. Paths (prereq: 3, `cycles`)

| Capability | Corpus |
|---|---|
| fixed-length hops (`*1..3`) | graphs |
| bounded variable-length with a selective anchor | graphs |
| undirected/multi-direction traversals | graphs |
| shortest path (SHORTEST k / ALL SHORTEST / shortestPath()) | graphs |
| quantified path patterns (`(){2,3}`, alternation, grouping) — head variables of a group must be read through the path (`nodes(p)[0].x`), not `a.x` (LABS-268) | graphs |
| traversal modes (WALK/TRAIL/ACYCLIC/SIMPLE/DIFFERENT RELATIONSHIPS) | graphs |

### 5. Writes (prereq: 3)

| Capability | Corpus |
|---|---|
| CREATE literal + computed props | clauses/create |
| MERGE + ON CREATE/ON MATCH | clauses/merge |
| SET / REMOVE round trips (prop, label) | clauses/create, remove |
| DELETE (+ DETACH) of self-created nodes | clauses/delete |
| FOREACH-driven writes | clauses/create |
| FOR loops (Cypher 25) | — |
| WHEN…DO conditional writes (Cypher 25) | — |
| read-computed writes (SET/CREATE from an aggregation) | clauses/* |

Write rounds grow the dataset deliberately: every write uses a `__wl_` synthetic key
and is deleted by its own or the next entry; the validator's leak check proves the
dataset is untouched.

### 6. Search and scale (prereq: 3)

| Capability | Needs |
|---|---|
| full-text index query (db.index.fulltext.queryNodes / SEARCH … FULLTEXT) | text-index |
| vector index query (db.index.vector.queryNodes / SEARCH … VECTOR) + property-filter pushdown | vector-index |
| LOAD CSV | a file: URL |
| EXPLAIN/PROFILE wrappers | engine support |

## Tier anchors (structural, not negotiated)

- **simple** — one pattern, optional predicate, projection, ordering/paging.
- **mid** — adds exactly one of: WITH chain, aggregation, OPTIONAL MATCH, subquery,
  CASE, UNION, comprehension, 2–3-hop traversal, one self-cleaning write.
- **complex** — adds any of: bounded variable-length/quantified/shortest paths on
  real cycles, nested subqueries, read-computed writes, Cypher 25 constructs.
  A *fixed* hop count (`*2`, `*1..3` fully fixed) is never complex by itself.
