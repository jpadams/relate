// r035: ( A . B ) + [SIMPLE] — (likes.hasCreator)+ (seed label Person, source seed p3378)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: kept — timeout at SF0.1 (unbounded cyclic recursion; the mode-prefix cliff itself is closed by ADR-0471, the unbounded class is not); frontier-NFA acceptance case (LABS-644 family)
MATCH p = SIMPLE (x:Person)((a)-[:likeComment|likePost]-(b)-[:commentHasCreator|postHasCreator]-(c)){1,}(y) WHERE x.ID = $seed RETURN p LIMIT 100
