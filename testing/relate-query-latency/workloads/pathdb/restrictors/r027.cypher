// r027: ( A | B ) [WALK] — likes|hasInterest (seed label Person, source seed p3378)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (324.0 ms)
MATCH p = WALK (x:Person)-[:likeComment|likePost|hasInterest]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
