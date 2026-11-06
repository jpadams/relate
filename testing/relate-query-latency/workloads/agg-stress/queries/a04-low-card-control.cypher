// a04: low-cardinality control — the SAME 1..3 walk stream and the SAME
// fold as a03, keyed on a ten-bucket attribute instead of the source id:
// the control that separates stream cost from group-count cost (10
// groups vs a03's 1199 over the identical 2,624,450-row stream; a fold
// that only speeds up group handling cannot move this entry much, one
// that speeds up row ingestion moves all of them)
// source: IN-HOUSE (2026-11-02), written for the LABS-733 aggregation-stress
// workload over the pinned graph-benchmark-ldbc SF0.1 store (Person/knows; see the workload
// README).
// oracle: oracle-sf0.1.json — the complete fold output, all ten buckets
// status: ok at SF0.1 (10 rows, 5.8 s — 2026-11-02 landing run, release,
// graph-benchmark-ldbc SF0.1)
MATCH (a:Person)-[:knows*1..3]->(b:Person)
RETURN a.ID % 10 AS bucket,
       count(*) AS paths,
       count(DISTINCT b.ID) AS distinctEndpoints
ORDER BY bucket
