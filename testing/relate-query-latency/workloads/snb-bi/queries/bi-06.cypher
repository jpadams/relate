// bi-06: Most authoritative users on a given topic
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-6.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (100 rows, ~0.3 s — 2026-11-02 landing run)
MATCH (tag:Tag {name: $tag})<-[:commentHasTag|postHasTag]-(message1:Comment|Post)-[:commentHasCreator|postHasCreator]->(person1:Person)
OPTIONAL MATCH (message1)<-[:likeComment|likePost]-(person2:Person)
OPTIONAL MATCH (person2)<-[:commentHasCreator|postHasCreator]-(message2:Comment|Post)<-[like:likeComment|likePost]-(person3:Person)
RETURN
  person1.ID AS personId,
  // DISTINCT like: each person2's popularity score is only added once per person1
  count(DISTINCT like) AS authorityScore
ORDER BY
  authorityScore DESC,
  person1.ID ASC
LIMIT 100
