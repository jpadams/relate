// r022: ( A . B ) [WALK] — hasMember.likes (seed label Forum, source seed f36)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (4.4 ms, re-measured 2026-11-01 — a bounded pattern; the copied unbounded-recursion reason could not apply)
MATCH p = WALK (x:Forum)-[:hasMember]-()-[:likeComment|likePost]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
