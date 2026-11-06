// r03: Denmark Hill -> Gatwick Airport, fewest LINK hops (ANY SHORTEST)
// source: IN-HOUSE. The 15-hop shortest route was first found by the
// 2026-10-31 spike and geographically verified (the Brighton main line
// approach via Brixton, Crystal Palace and East Croydon); this query text was
// re-verified by execution 2026-11-02 against the pinned dogofbrian/ukrailway
// data (@ ce82229e547f46ef4dd0c680f42756e17bde8922, Apache-2.0, fetched and
// checksummed — see the workload README): 15 hops over the station LINK
// graph.
// oracle: oracle.json — exactly one row, hops = 15 and the 15-hop station
// list (Denmark Hill .. Gatwick Airport)
// status: ok — milliseconds-scale over the imported graph (the ANY SHORTEST
// walk anchors on the name index; ~4 ms first, ~3 ms median in-lane)
MATCH p = ANY SHORTEST
  (dmk:Station {name: 'Denmark Hill'}) (()-[:LINK]-())+ (gtw:Station {name: 'Gatwick Airport'})
RETURN length(p) AS hops,
       [n IN nodes(p) WHERE n:Station | n.name] AS stations
