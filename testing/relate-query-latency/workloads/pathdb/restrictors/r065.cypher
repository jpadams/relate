// r065: ( ( A | B ) | C ) [SIMPLE] — (workAt | knows) | likes (seed label Person, source seed p10)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (346.1 ms)
MATCH p = SIMPLE (x:Person)-[:workAt|knows|likeComment|likePost]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
