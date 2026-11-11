# Expression-form shortestPath divergence study

Status: research memo; feeds the one-shortest-path-engine unification.
Related: ADR-0186, ADR-0189, ADR-0025.

## Verdict

**GO for unification, with one narrow carve-out.** The divergence is not
a TCK or language contract. It is a documented, test-pinned, but
accidental-by-construction artefact of the expression form returning one
value per input row. The unified engine does not have to replicate
"first start candidate in candidate order" for `shortestPath`. It must
keep the expression form's row contract (one value per input row, `null`
when nothing qualifies, a list for `allShortestPaths`), and it must keep
its own tests honest by deliberately rewriting the three pins listed
under "What pins it".

The maintainer ruling that which equally short trail is returned is
unspecified already covers witness order. This memo covers the remaining
question: which trails a multi-candidate expression returns.

## The premise needs one correction

The route does not take the shortest over all candidates. It answers per
(start, end) partition (`Route::run`: `pair_partition` per pair, `fan`
per origin, `Loop` per start). `shortestPath` as a `MATCH` clause returns
one row per connected (start, end) pair; `allShortestPaths` one row per
shortest trail of each pair (documented in `docs/reference/cypher-syntax.md`).
The route and forced GPM enumeration agree on this (route parity tests).

So the two forms do not differ in "which candidate wins". They differ in
**cardinality**: the clause emits a row per pair, the expression emits
exactly one value per input row and has to collapse the pairs.

## What differs

Graph used by the differential tests: starts `s1`, `s2` (`:S`), ends
`t1`, `t2` (`:T`); `s1` reaches `t1` in 3 hops, `s2` reaches `t1` in 1
and `t2` in 2.

| # | Case | Clause rows (start, end, hops) | Expression value |
| --- | --- | --- | --- |
| 1 | `shortestPath((:S)-[*]->(:T))`, `min_hops` <= 1 | (s1,t1,3) (s2,t1,1) (s2,t2,2) | the first start candidate that has a trail: s1, 3 hops. Not the global minimum (1) |
| 2 | same, `s2` created first | unchanged | s2, 1 hop: the result depends on candidate order |
| 3 | `allShortestPaths`, same pattern | all three pairs | first candidate sets the length: s1's 3-hop trail only. Later candidates add trails of exactly that length |
| 4 | `allShortestPaths`, `s2` first | unchanged | s2's 1-hop trail only; s1's 3-hop trail dropped |
| 5 | bound-by-pattern end `(:T {id: 't1'})` | (s1,t1,3) (s2,t1,1) | first candidate again (3 hops) |
| 6 | `min_hops` 2 (`[*2..]`) | per pair | enumeration: shortest over **all** candidates together, order independent (2 hops, s2 to t2) |
| 7 | `min_hops` 0, several starts matching the end | one zero-length row per start | `shortestPath`: first start's zero-length path. `allShortestPaths`: every matching start adds its own zero-length path, because that arm takes no length cap |
| 8 | start already bound by an earlier clause | one row per (start, end) | one value per input row: shallowest end layer per start. The forms agree per start apart from cardinality over several ends |
| 9 | closed trail, `(s)-[*1..]->(s)`, `s` bound per row | shortest closed trail per start | same, per row. No divergence |
| 10 | one start and one end candidate | the pair's shortest | identical |

Rows 1 to 5 and 7 are where candidate order is observable. Row 6 is the
opposite behaviour inside the same form: an expression with
`min_hops` > 1 already takes the global shortest. Therefore the
expression form is currently inconsistent with itself across `min_hops`,
which is evidence the candidate-order behaviour is not a designed
contract.

The unbound-end layering inside one start (the first layer that holds a
matching end; the start itself matching the end merges its closed trails
by shorter-wins) is shared with the clause per partition and is not a
divergence.

## What pins it

- **TCK / spec harness.** The TCK lives in the spec harness, not in this
  repository, and the in-repo pinned corpus is retired (ADR-0520); the
  repository holds no TCK expectation for `shortestPath`. The upstream
  openCypher TCK does not specify multi-candidate `shortestPath`
  (it is a vendor extension there; Neo4j's own semantics is per pair in
  `MATCH`, and the expression form is not covered). We found no
  contract source for candidate order. Not re-run here: the spec harness
  was not exercised for this memo.
- **Documentation.** `docs/reference/cypher-syntax.md` states only the
  row contract (one value per input row, `null` when unreachable); it
  does not promise candidate-order selection.
- **ADR-0189.** Records the behaviour as "unchanged for `min_hops` <= 1"
  and names the later-candidate `allShortestPaths` change as deliberate.
  It does not claim the order is a contract; it is a preservation
  decision made to keep the review narrow.
- **Unit test.** `start_candidates_combine_in_candidate_order`
  (`shortest_expression_tests.rs`) pins rows 1 and 3 on a three-start
  graph. This is the only pin. It must change with a unification that
  takes the shortest over all candidates.
- **New differential tests** (`shortest_divergence_tests.rs`) pin the
  table above, including the clause side, so a unification changes them
  knowingly.

## What the unified engine must replicate

Must keep:

1. One value per input row; `null` when no trail qualifies; a list for
   `allShortestPaths`; the 32-hop default for unbounded `[*]` in the
   expression form (the clause route has no hop cap).
2. Closed-trail behaviour when start equals end (row 9) and the
   unbound-end merge of closed and open trails.
3. The `min_hops` 0 zero-length path when start matches end.
4. Exact enumeration semantics for `min_hops` > 1 (ADR-0189).

Free to change (maintainer decision, not forced by any contract):

5. `shortestPath` over several start candidates: returning the global
   minimum over all candidates (as `min_hops` > 1 already does) removes
   the order dependence. Which witness among equals stays unspecified.
   Cost: the expression form must search every start candidate instead of
   stopping at the first with a trail. Today's early exit is a
   performance feature for large candidate sets; the memo does not
   measure it (no timed runs were permitted).
6. `allShortestPaths` over several candidates: "all trails of the global
   minimum length" is the natural unification. It also removes the
   row 7 anomaly and the later-candidate length-cap quirk.

The carve-out is point 5: if the unified engine keeps first-candidate
early exit for speed, the early exit must be stated in the spec as
"unspecified which start candidate", not be left as an accident of
iteration order. Either way this is an ADR-worthy dialect decision and
needs the maintainers' ruling before implementation.

## Risk assessment

- Behavioural risk of switching to global-minimum: queries with several
  start candidates and no tie-break already return an arbitrary start's
  trail; they gain determinism by length, but a query that happened to
  get a longer trail from the first candidate now gets a shorter one.
  No in-repo test other than the one above depends on the old result.
- Baseline loadtests use bound start and bound end (ADR-0189
  Consequences), a single candidate each, so their semantics and, for
  the pair search, their cost are unaffected.

## Verification status

The differential tests were drafted without a toolchain run: the
machine was reserved for a timed benchmark partway through the work, so
the expected values come from reading `eval_walk/shortest.rs` and the
route, not from execution. A single compile before the ready flip is
required: `cargo test -p relate-executor --lib shortest_divergence`.
