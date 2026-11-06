// r003: A ? [WALK-RE] — likes? (seed label Person, source seed p3378)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (318.7 ms)
MATCH REPEATABLE ELEMENTS p = WALK (x:Person)-[:likeComment|likePost*0..1]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
