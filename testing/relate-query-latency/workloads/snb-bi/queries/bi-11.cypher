// bi-11: Friend triangles
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-11.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (1 row, ~13 ms — 2026-11-02 landing run)
MATCH (a:Person)-[:personIsLocatedIn]->(:Place)-[:isPartOf]->(country:Place {name: $country}),
      (a)-[k1:knows]-(b:Person)
WHERE a.ID < b.ID
  AND $startDate <= datetime(k1.creationDate) AND datetime(k1.creationDate) <= $endDate
WITH DISTINCT country, a, b
MATCH (b)-[:personIsLocatedIn]->(:Place)-[:isPartOf]->(country)
WITH DISTINCT country, a, b
MATCH (b)-[k2:knows]-(c:Person),
      (c)-[:personIsLocatedIn]->(:Place)-[:isPartOf]->(country)
WHERE b.ID < c.ID
  AND $startDate <= datetime(k2.creationDate) AND datetime(k2.creationDate) <= $endDate
WITH DISTINCT a, b, c
MATCH (c)-[k3:knows]-(a)
WHERE $startDate <= datetime(k3.creationDate) AND datetime(k3.creationDate) <= $endDate
WITH DISTINCT a, b, c
RETURN count(*) AS count
