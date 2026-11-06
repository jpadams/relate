// r020: A* [SIMPLE] — knows* (seed label Person, source seed p84)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: kept — timeout at SF0.1 (unbounded cyclic recursion; the mode-prefix cliff itself is closed by ADR-0471, the unbounded class is not); frontier-NFA acceptance case (LABS-644 family)
MATCH p = SIMPLE (x:Person)-[:knows*0..]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
