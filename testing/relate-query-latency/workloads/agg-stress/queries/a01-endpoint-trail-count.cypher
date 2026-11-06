// a01: trail-count endpoint fold — every bounded knows walk, folded per
// destination (the workload's headline: the fold's input stream is the
// walk enumeration itself, exponential-ish in the bound)
// source: IN-HOUSE (2026-11-02), written for the LABS-733 aggregation-stress
// workload over the pinned graph-benchmark-ldbc SF0.1 store (Person/knows, the same data the
// graph-benchmark-ldbc, pathdb and snb-bi lanes run; see the workload README). The bound is
// 1..3: measured at SF0.1 the 1..4 fold enumerates 23,050,339 walks in
// ~6.7 minutes under memory pressure (61 s system time) — minutes-scale,
// past the workload's seconds-to-tens-of-seconds budget; 1..3 enumerates
// 2,624,450 walks (the README records the measurement).
// oracle: oracle-sf0.1.json — the ten most-walked endpoints and their path
// counts (1205 endpoint groups in all)
// status: ok at SF0.1 (10 rows, 10.3 s — 2026-11-02 landing run, release,
// graph-benchmark-ldbc SF0.1)
MATCH p = (a:Person)-[:knows*1..3]->(b:Person)
RETURN b.ID AS endpoint, count(p) AS paths
ORDER BY paths DESC, endpoint
LIMIT 10
