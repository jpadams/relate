// p122: ( A | B+ ) with A = B — isSubclassOf | (isSubclassOf+) (seed label Tagclass, source seed tc0); collapsed to isSubclassOf+ — ( A | A+ ) ≡ A+
// source: dbgutalca/pathdb-eval1 @ e62e33c2a40aee3cecca35961dd6da94a575c4f1, Queries.xlsx (Apache-2.0), mechanical QPP translation (2026-11-01 spike)
// status: ok at SF0.1 (spike 2026-11-01; re-measured 2026-11-01 after the same-type alternation collapse — same answer)
MATCH p = (x:Tagclass)-[:isSubclassOf*1..]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
