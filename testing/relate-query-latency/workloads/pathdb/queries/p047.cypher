// p047: ( A | B ) + — (hasCreator | isLocatedIn)+ (seed label Comment, source seed m1003606)
// source: dbgutalca/pathdb-eval1 @ e62e33c2a40aee3cecca35961dd6da94a575c4f1, Queries.xlsx (Apache-2.0), mechanical QPP translation (2026-11-01 spike)
// status: kept — timeout at SF0.1 (cyclic recursion class); frontier-NFA acceptance case (LABS-644 family)
MATCH p = (x:Comment)-[:commentHasCreator|postHasCreator|commentIsLocatedIn|organisationIsLocatedIn|personIsLocatedIn|postIsLocatedIn*1..]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
