// bi-05: Most active posters of a given topic
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-5.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (38 rows, ~2 ms — 2026-11-02 landing run)
MATCH (tag:Tag {name: $tag})<-[:commentHasTag|postHasTag]-(message:Comment|Post)-[:commentHasCreator|postHasCreator]->(person:Person)
OPTIONAL MATCH (message)<-[likes:likeComment|likePost]-(:Person)
WITH person, message, count(likes) AS likeCount
OPTIONAL MATCH (message)<-[:replyOfComment|replyOfPost]-(reply:Comment)
WITH person, message, likeCount, count(reply) AS replyCount
WITH person, count(message) AS messageCount, sum(likeCount) AS likeCount, sum(replyCount) AS replyCount
RETURN
  person.ID AS personId,
  replyCount,
  likeCount,
  messageCount,
  1*messageCount + 2*replyCount + 10*likeCount AS score
ORDER BY
  score DESC,
  person.ID ASC
LIMIT 100
