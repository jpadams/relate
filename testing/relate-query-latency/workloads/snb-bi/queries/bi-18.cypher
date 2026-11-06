// bi-18: Friend recommendation
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-18.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (20 rows, ~2.0 s — 2026-11-02 landing run)
MATCH (tag:Tag {name: $tag})<-[:hasInterest]-(person1:Person)-[:knows]-(mutualFriend:Person)-[:knows]-(person2:Person)-[:hasInterest]->(tag)
WHERE person1 <> person2
  AND NOT (person1)-[:knows]-(person2)
RETURN person1.ID AS person1Id, person2.ID AS person2Id, count(DISTINCT mutualFriend) AS mutualFriendCount
ORDER BY mutualFriendCount DESC, person1Id ASC, person2Id ASC
LIMIT 20
