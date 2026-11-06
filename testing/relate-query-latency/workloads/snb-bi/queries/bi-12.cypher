// bi-12: How many persons have a given number of posts
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-12.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: kept — times out at SF0.1 (cancelled at 60 s in the landing probe): the OPTIONAL unbounded replyOfComment|replyOfPost*0.. walk per person over all messages — the walk-budget class, LABS-660
MATCH (person:Person)
OPTIONAL MATCH (person)<-[:commentHasCreator|postHasCreator]-(message:Comment|Post)-[:replyOfComment|replyOfPost*0..]->(post:Post)
WHERE message.content IS NOT NULL
  AND message.length < $lengthThreshold
  AND datetime(message.creationDate) > $startDate
  AND post.language IN $languages
WITH
  person,
  count(message) AS messageCount
RETURN
  messageCount,
  count(person) AS personCount
ORDER BY
  personCount DESC,
  messageCount DESC
