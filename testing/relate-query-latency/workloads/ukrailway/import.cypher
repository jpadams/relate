// The UK railway import script: loads the fetched dogofbrian/ukrailway
// data set (spike-verified 2026-10-31, lane-verified 2026-11-02) into the
// lane's :memory: store through the engine's LOAD CSV.
// Source data: the pinned dogofbrian/ukrailway data set (Apache-2.0),
// data/*.csv — fetched on demand, never committed. The pin and its
// checksums are quoted by reference, never by value: fetch.rs
// PINNED_COMMIT (the one hash spelling in the tree) /
// workloads/ukrailway/data.sha256.
// Every LOAD CSV source below is root-relative: the store's CSV policy
// (ADR-0081) resolves it under the fetched data directory the lane opens
// the store with, and confines every read to exactly that directory —
// no path is spelled in the script itself.
//
// Two spike findings are baked in:
// - one station (East Usk Jn.) has empty lat/long: the CASE guard yields null
//   where a bare point({latitude: null}) would type-error (Neo4j yields null);
// - stops.csv is 7.59 MiB against the 8 MiB LOAD CSV cap (5% headroom) — if the
//   cap is ever lowered, this file splits stops.csv in two.

LOAD CSV WITH HEADERS FROM 'stations.csv' AS row
CREATE (s:Station {id: toInteger(row.id), name: row.name, tiplocCode: row.tiplocCode, crsCode: row.crsCode,
  location: CASE WHEN row.latitude = '' OR row.longitude = '' THEN null ELSE point({latitude: toFloat(row.latitude), longitude: toFloat(row.longitude)}) END});

LOAD CSV WITH HEADERS FROM 'stations.csv' AS row
WITH row WHERE row.groupName = 'LONDON GROUP'
MATCH (s:Station {id: toInteger(row.id)})
SET s:LondonGroup;

CREATE INDEX station_id IF NOT EXISTS FOR (s:Station) ON (s.id);
CREATE INDEX station_name IF NOT EXISTS FOR (s:Station) ON (s.name);

LOAD CSV WITH HEADERS FROM 'calling_points.csv' AS row
CREATE (c:CallingPoint {id: toInteger(row.id), routeName: row.routeName, stopNumber: toInteger(row.stopNumber)});

CREATE INDEX calling_point_id IF NOT EXISTS FOR (c:CallingPoint) ON (c.id);

LOAD CSV WITH HEADERS FROM 'stops.csv' AS row
CREATE (st:Stop {id: toInteger(row.id), stopNumber: toInteger(row.stopNumber),
  arrives: CASE WHEN row.arrives = '' THEN null ELSE time(row.arrives) END,
  departs: CASE WHEN row.departs = '' THEN null ELSE time(row.departs) END});

CREATE INDEX stop_id IF NOT EXISTS FOR (st:Stop) ON (st.id);

LOAD CSV WITH HEADERS FROM 'links.csv' AS row
MATCH (a:Station {id: toInteger(row.source)})
MATCH (b:Station {id: toInteger(row.target)})
CREATE (a)-[:LINK {distance: toFloat(row.distance)}]->(b);

LOAD CSV WITH HEADERS FROM 'calling_point_seq.csv' AS row
MATCH (a:CallingPoint {id: toInteger(row.source)})
MATCH (b:CallingPoint {id: toInteger(row.target)})
CREATE (a)-[:NEXT]->(b);

LOAD CSV WITH HEADERS FROM 'stop_seq.csv' AS row
MATCH (a:Stop {id: toInteger(row.source)})
MATCH (b:Stop {id: toInteger(row.target)})
CREATE (a)-[:NEXT]->(b);

LOAD CSV WITH HEADERS FROM 'calls_at.csv' AS row
MATCH (a:CallingPoint {id: toInteger(row.source)})
MATCH (b:Station {id: toInteger(row.target)})
CREATE (a)-[:CALLS_AT]->(b);

LOAD CSV WITH HEADERS FROM 'calling_point_stops.csv' AS row
MATCH (a:CallingPoint {id: toInteger(row.source)})
MATCH (b:Stop {id: toInteger(row.target)})
CREATE (a)-[:HAS]->(b);
