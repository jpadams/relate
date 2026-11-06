// r010: A+ [SIMPLE] — replyOf+ (seed label Comment, source seed m1003614)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: ok — re-measured 2026-11-01: ~85 ms, 20 rows at SF0.1 (the replyOf forest
//   is acyclic, so the unbounded quantifier under the mode prefix terminates — the
//   copied cyclic-recursion kept-reason could not apply to this data)
MATCH p = SIMPLE (x:Comment)-[:replyOfComment|replyOfPost*1..]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
