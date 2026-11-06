// r061: ( ( A | B ) | C ) [TRAIL] — (workAt | knows) | likes (seed label Person, source seed p10)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (354.6 ms)
MATCH p = TRAIL (x:Person)-[:workAt|knows|likeComment|likePost]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
