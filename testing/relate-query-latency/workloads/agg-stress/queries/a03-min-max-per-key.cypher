// a03: min/max per key over a big stream — the same 1..3 walk stream
// folded per source over the destinations' creationDate extremes (raw
// ISO strings: uniform format, so lexicographic order is temporal
// order), with the arrival count beside them
// source: IN-HOUSE (2026-11-02), written for the LABS-733 aggregation-stress
// workload over the pinned graph-benchmark-ldbc SF0.1 store (Person/knows; see the workload
// README). No path variable: the frontier declines it, so the stream is
// the walk enumeration without path values (a01 carries the with-path
// cost of the same bound).
// oracle: oracle-sf0.1.json — the first ten sources in key order with
// their fold output (1199 source groups in all)
// status: ok at SF0.1 (10 rows, 6.1 s — 2026-11-02 landing run, release,
// graph-benchmark-ldbc SF0.1)
MATCH (a:Person)-[:knows*1..3]->(b:Person)
RETURN a.ID AS src,
       min(b.creationDate) AS firstCreated,
       max(b.creationDate) AS lastCreated,
       count(b) AS arrivals
ORDER BY src
LIMIT 10
