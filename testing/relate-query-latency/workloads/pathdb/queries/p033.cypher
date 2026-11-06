// p033: ( A . B ) ? — (containerOf.hasCreator)? (seed label Forum, source seed f40)
// source: dbgutalca/pathdb-eval1 @ e62e33c2a40aee3cecca35961dd6da94a575c4f1, Queries.xlsx (Apache-2.0), mechanical QPP translation (2026-11-01 spike)
// status: ok at SF0.1 (spike 2026-11-01)
MATCH p = (x:Forum)-[:containerOf]-()-[:commentHasCreator|postHasCreator]-(y) WHERE x.ID = $seed RETURN p LIMIT 100 UNION MATCH p = (x:Forum) WHERE x.ID = $seed RETURN p LIMIT 100
