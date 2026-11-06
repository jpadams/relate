// r056: ( ( A . B ) . C ) [TRAIL] — knows.knows.workAt (seed label Person, source seed p84)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (39.3 ms, re-measured 2026-11-01 — a bounded pattern; the copied unbounded-recursion reason could not apply)
MATCH p = TRAIL (x:Person)-[:knows]-()-[:knows]-()-[:workAt]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
