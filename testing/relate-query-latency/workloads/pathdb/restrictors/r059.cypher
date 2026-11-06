// r059: ( ( A . B ) . C ) [ACYCLIC] — knows.knows.workAt (seed label Person, source seed p84)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok at SF0.1 (9401.3 ms)
MATCH p = ACYCLIC (x:Person)-[:knows]-()-[:knows]-()-[:workAt]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
