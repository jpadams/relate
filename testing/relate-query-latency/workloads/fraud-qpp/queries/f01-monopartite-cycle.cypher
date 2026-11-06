// f01: directed transaction rings, 2-6 hops, monopartite model
// source: IN-HOUSE, re-authored (2026-11-01). The query shape is the fraud-ring
// detection pattern; the external repos that inspired it are unlicensed and were
// never copied (see license-notes.md). Runs over the existing
// relate-datasets fraud generator (Account/TRANSFER, planted 'cycle').
// status: ok — the planted-ring oracle check (oracle.json); runs over the
// generated graph (fraud-generate's Account/TRANSFER model, seed 42 at the
// datasets crate's CI scale)
MATCH path = (a:Account {planted: 'cycle'})-[:TRANSFER*2..6]->(a)
RETURN a.id AS account, [n IN nodes(path) | n.id] AS ring
