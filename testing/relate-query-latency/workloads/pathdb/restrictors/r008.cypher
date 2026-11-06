// r008: A+ [WALK-RE] — replyOf+ (seed label Comment, source seed m1003614)
// source: restrictor matrix re-spelling of the PathDB template (faithful path-mode spelling); spike 2026-11-01
// status: error-pin UnsafeRepeatableElements — authored fail-closed (REPEATABLE ELEMENTS cannot combine with an unbounded quantified path pattern)
MATCH REPEATABLE ELEMENTS p = WALK (x:Comment)-[:replyOfComment|replyOfPost*1..]-(y) WHERE x.ID = $seed RETURN p LIMIT 100
