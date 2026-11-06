// bi-07: Related topics
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-7.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (100 rows, ~15 ms — 2026-11-02 landing run)
MATCH
  (tag:Tag {name: $tag})<-[:commentHasTag|postHasTag]-(message:Comment|Post),
  (message)<-[:replyOfComment|replyOfPost]-(comment:Comment)-[:commentHasTag]->(relatedTag:Tag)
WHERE NOT (comment)-[:commentHasTag]->(tag)
RETURN
  relatedTag.name,
  count(DISTINCT comment) AS count
ORDER BY
  count DESC,
  relatedTag.name ASC
LIMIT 100
