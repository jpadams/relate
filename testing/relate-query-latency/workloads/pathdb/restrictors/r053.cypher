// r053: ( A? | B ) [WALK-RE] — (knows ? ) | likes (seed label Person, source seed p84)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (324.3 ms)
MATCH REPEATABLE ELEMENTS p = WALK (x:Person)-[:knows*0..1]-(y) WHERE x.ID = $seed RETURN p LIMIT 100 UNION MATCH REPEATABLE ELEMENTS p = WALK (x:Person)-[:likeComment|likePost]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
