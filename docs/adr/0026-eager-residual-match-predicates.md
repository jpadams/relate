# 0026. Residual MATCH predicates apply when their variables bind

- **Status**: Proposed
- **Date**: 2026-08-31
- **Deciders**: MemoryDB maintainers

## Context

ADR-0024 folds node equality / labels into the pattern map and orients
the path so `lookup_nodes` seeks. Residual `WHERE` still runs after
`match_paths` produces every trail. A mid-path inequality
(`t.amount > $min` on the first hop of a two-hop MATCH) therefore
finishes the cartesian expand and throws the hops away.

Relationship equality in `WHERE` (`r.k = literal|$param`) is not folded
today; the executor already checks a `RelPattern` property map during
`walk`. A LogicalOp tree is out of scope (same rejection as ADR-0024).

## Decision

Residual MATCH / OPTIONAL MATCH `WHERE` conjuncts (top-level `AND`
leaves) are evaluated **as soon as every free variable they mention is
bound**, including variables bound by an earlier clause. They are not
held until the path is complete.

AND-equality `r.k = literal|$param` (either side) on a relationship
**introduced in this MATCH** is folded into that relationship’s property
map, the same way ADR-0024 folds nodes. The existing `walk` property
check is the access method.

Keep ADR-0024 barriers: no rewrite of `CREATE` / `MERGE` / `DELETE` /
`SET`; no reorder across `WITH` or `UNWIND`; no `OR` / `XOR` fold into
IndexSeek; no cost model; no range or relationship-type indexes. Do not
move `WITH` WHERE into MATCH.

OPTIONAL MATCH emptiness is unchanged: if every path fails a residual
conjunct, emit the null-extended row.

## Alternatives considered

- **Keep residual WHERE after the full path** — correct, two-hop
  filters still expand the second hop. Rejected; that is the remaining
  ADR-0009 pushdown.
- **LogicalOp Filter nodes** — larger TCK surface for the same
  `eval` + `walk` access methods. Rejected (ADR-0024).
- **Fold inequalities into a range index** — no range index exists
  (ADR-0010). Rejected; evaluate the conjunct when the variable binds.

## Consequences

- A first-hop residual can prune before later expands. Result multisets
  stay the same as late `retain`.
- Predicates on pre-MATCH variables become seed-time filters. They are
  still not folded into an IndexSeek (ADR-0024).
- `WITH` WHERE still sees pre-projection names (TCK).
- Pattern-predicate / `EXISTS` matching should use the same
  label+equality seek as MATCH seed (implementation; not a new access
  method).

## Later references

ADR-0030: `CONTAINS` / `STARTS WITH` AND-leaves on a MATCH-introduced
node seed from prefix/trigram postings; residual still verifies.
`OR` / inequalities are still not IndexSeek.

ADR-0031: typed adjacency is the expand access method; residual type
checks are skipped when the partition is already that type.
