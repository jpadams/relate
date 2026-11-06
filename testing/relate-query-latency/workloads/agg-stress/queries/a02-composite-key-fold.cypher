// a02: composite-key fold — the walk stream folded per (source,
// destination) pair with count and per-key min/max/avg of the hop count:
// the high-cardinality case (391,109 groups at SF0.1; the LABS-685
// follow-on's 4-6x estimate is for exactly this shape)
// source: IN-HOUSE (2026-11-02), written for the LABS-733 aggregation-stress
// workload over the pinned graph-benchmark-ldbc SF0.1 store (Person/knows; see the workload
// README). avg(length(p)) records exactly (halves over the recorded rows),
// so the oracle's floats are bit-stable.
// oracle: oracle-sf0.1.json — the first ten (src, dst) pairs in key order
// with their fold output
// status: ok at SF0.1 (10 rows, 15.3 s — 2026-11-02 landing run, release,
// graph-benchmark-ldbc SF0.1)
MATCH p = (a:Person)-[:knows*1..3]->(b:Person)
RETURN a.ID AS src, b.ID AS dst,
       count(p) AS paths,
       min(length(p)) AS minHops,
       max(length(p)) AS maxHops,
       avg(length(p)) AS avgHops
ORDER BY src, dst
LIMIT 10
