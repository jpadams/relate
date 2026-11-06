// bi-09: Top thread initiators
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-9.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: kept — times out at SF0.1 (cancelled at 60 s in the landing probe): the unbounded replyOfComment|replyOfPost*0.. walk per thread-initiator post — the walk-budget class, LABS-660
MATCH (person:Person)<-[:postHasCreator]-(post:Post)<-[:replyOfComment|replyOfPost*0..]-(reply:Comment|Post)
WHERE  datetime(post.creationDate) >= $startDate
  AND  datetime(post.creationDate) <= $endDate
  AND  datetime(reply.creationDate) >= $startDate
  AND  datetime(reply.creationDate) <= $endDate
RETURN
  person.ID AS personId,
  person.firstName,
  person.lastName,
  count(DISTINCT post) AS threadCount,
  count(DISTINCT reply) AS messageCount
ORDER BY
  messageCount DESC,
  person.ID ASC
LIMIT 100
