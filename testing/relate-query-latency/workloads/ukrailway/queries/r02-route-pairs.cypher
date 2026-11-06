// r02: connected (origin, destination) station pairs over calling-point
// chains, and the stations the two legs change at
// source: IN-HOUSE re-derivation. The values were first verified by the
// 2026-10-31 spike against the pinned dogofbrian/ukrailway data
// (@ ce82229e547f46ef4dd0c680f42756e17bde8922, Apache-2.0, fetched and
// checksummed — see the workload README); this query text was re-verified by
// execution 2026-11-02: routePairs = 1248, changeoverStations = 19. (The
// earlier draft counted DISTINCT changeover calling points under the key
// changeoverCallingPoints — the verified query counts DISTINCT changeover
// STATIONS, and the expected key is changeoverStations.)
// oracle: oracle.json — exactly one row, routePairs = 1248,
// changeoverStations = 19
// status: ok — minutes-scale over the imported graph (the README records the
// timing); runs under the lane's own default timeout, never the
// scale-factor-scaled default
MATCH (dmk:Station {name: 'Denmark Hill'})<-[:CALLS_AT]-(l1:CallingPoint)
      (()-[:NEXT]->())+
      (:CallingPoint)-[:CALLS_AT]->(x:Station)<-[:CALLS_AT]-(l2:CallingPoint)
      (()-[:NEXT]->())+
      (:CallingPoint)-[:CALLS_AT]->(gtw:Station {name: 'Gatwick Airport'})
RETURN count(*) AS routePairs,
       count(DISTINCT x) AS changeoverStations
