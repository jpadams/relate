// f02: non-node-repeating rings via the SIMPLE path mode (native restrictor; the
// external training material emulated this with APOC/COUNT{} — the path-mode
// spelling is the Cypher-25 form our dialect ships)
// source: IN-HOUSE, re-authored
// status: ok — the planted-ring oracle check (oracle.json); runs over the
// generated graph, like f01. Anchored on the planted cycle like f01 (f01 is
// the same anchor under the default mode): the unanchored spelling enumerates
// every simple cycle in the generated noise — output-exponential at any
// interesting scale, a property of the data, not an engine defect — while the
// anchored one tests the same SIMPLE restrictor against the planted ring.
MATCH p = SIMPLE (a:Account {planted: 'cycle'})-[:TRANSFER*2..6]->(a)
RETURN a.id AS account, [n IN nodes(p) | n.id] AS ring
