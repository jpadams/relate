// p095: ( A? | B ) with A = B — (hasInterest ?) | hasInterest (seed label Comment, source seed m1003606); collapsed to hasInterest*0..1 — ( A? | A ) ≡ A?, the UNION device is for A ≠ B arms
// source: dbgutalca/pathdb-eval1 @ e62e33c2a40aee3cecca35961dd6da94a575c4f1, Queries.xlsx (Apache-2.0), mechanical QPP translation (2026-11-01 spike)
// status: ok — workload anomaly: PathDB seeds a Comment for this template, which no LDBC
//   output carries (unreachable in its own data too); the seed key records the label the
//   policy resolved (Tag), so the Comment anchor never matches — 0 rows, as in PathDB's own
//   run (re-measured 2026-11-01 after the same-type alternation collapse)
MATCH p = (x:Comment)-[:hasInterest*0..1]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
