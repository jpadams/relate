// r029: ( A | B ) [ACYCLIC] — likes|hasInterest (seed label Person, source seed p3378)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (326.3 ms)
MATCH p = ACYCLIC (x:Person)-[:likeComment|likePost|hasInterest]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
