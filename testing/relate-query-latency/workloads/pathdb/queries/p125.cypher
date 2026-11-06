// p125: ( A | B* ) — hasCreator | (knows*) (seed label Comment, source seed m1003606)
// source: dbgutalca/pathdb-eval1 @ e62e33c2a40aee3cecca35961dd6da94a575c4f1, Queries.xlsx (Apache-2.0), mechanical QPP translation (2026-11-01 spike)
// status: ok at SF0.1 (spike 2026-11-01)
MATCH p = (x:Comment)-[:commentHasCreator|postHasCreator]-(y) WHERE x.ID = $seed RETURN p LIMIT 100 UNION MATCH p = (x:Comment)-[:knows*0..]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
