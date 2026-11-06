// r030: ( A | B ) [SIMPLE] — likes|hasInterest (seed label Person, source seed p3378)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (322.9 ms)
MATCH p = SIMPLE (x:Person)-[:likeComment|likePost|hasInterest]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
