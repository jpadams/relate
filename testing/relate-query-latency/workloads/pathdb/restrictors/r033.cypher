// r033: ( A . B ) + [WALK-RE] — (likes.hasCreator)+ (seed label Person, source seed p3378)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: error-pin UnsafeRepeatableElements — authored fail-closed (REPEATABLE ELEMENTS cannot combine with an unbounded quantified path pattern)
MATCH REPEATABLE ELEMENTS p = WALK (x:Person)((a)-[:likeComment|likePost]-(b)-[:commentHasCreator|postHasCreator]-(c)){1,}(y) WHERE x.ID = $seed RETURN p LIMIT 100
