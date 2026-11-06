// bi-02: Tag evolution
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-2.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (100 rows, ~0.13 s — 2026-11-02 landing run)
MATCH (tag:Tag)-[:hasType]->(:Tagclass {name: $tagClass})
// window 1
OPTIONAL MATCH (message1:Comment|Post)-[:commentHasTag|postHasTag]->(tag)
  WHERE $date <= datetime(message1.creationDate)
    AND datetime(message1.creationDate) < $date + duration({days: 100})
WITH tag, count(message1) AS countWindow1
// window 2
OPTIONAL MATCH (message2:Comment|Post)-[:commentHasTag|postHasTag]->(tag)
  WHERE $date + duration({days: 100}) <= datetime(message2.creationDate)
    AND datetime(message2.creationDate) < $date + duration({days: 200})
WITH
  tag,
  countWindow1,
  count(message2) AS countWindow2
RETURN
  tag.name,
  countWindow1,
  countWindow2,
  abs(countWindow1 - countWindow2) AS diff
ORDER BY
  diff DESC,
  tag.name ASC
LIMIT 100
