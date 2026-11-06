// a05: collect-heavy fold — the 1..2 walk stream folded per source with
// a list-accumulating aggregation (collect(DISTINCT destination)), sized
// by size() so the recorded answer stays deterministic (collect's own
// list order is not)
// source: IN-HOUSE (2026-11-02), written for the LABS-733 aggregation-stress
// workload over the pinned graph-benchmark-ldbc SF0.1 store (Person/knows; see the workload
// README). Memory stays bounded: 1199 per-source lists of at most ~875
// distinct ids over the 254,463-row stream.
// oracle: oracle-sf0.1.json — the ten widest-reach sources with their
// reach and walk counts
// status: ok at SF0.1 (10 rows, 0.57 s — 2026-11-02 landing run, release,
// graph-benchmark-ldbc SF0.1)
MATCH (a:Person)-[:knows*1..2]->(b:Person)
RETURN a.ID AS src,
       size(collect(DISTINCT b.ID)) AS reach,
       count(*) AS walks
ORDER BY reach DESC, src
LIMIT 10
