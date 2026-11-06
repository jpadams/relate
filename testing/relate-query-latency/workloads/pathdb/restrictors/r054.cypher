// r054: ( A? | B ) [ACYCLIC] — (knows ? ) | likes (seed label Person, source seed p84)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (325.4 ms)
MATCH p = ACYCLIC (x:Person)-[:knows*0..1]-(y) WHERE x.ID = $seed RETURN p LIMIT 100 UNION MATCH p = ACYCLIC (x:Person)-[:likeComment|likePost]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
