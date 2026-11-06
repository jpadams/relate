// bi-03: Popular topics in a country
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-3.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: kept — times out at SF0.1 (cancelled at 60 s in the landing probe, far past the 10 s CI bound): the unbounded replyOfComment|replyOfPost*0.. walk per country-anchored post — the walk-budget class, LABS-660
MATCH
  (:Place {name: $country})<-[:isPartOf]-(:Place)<-[:personIsLocatedIn]-
  (person:Person)<-[:hasModerator]-(forum:Forum)-[:containerOf]->
  (post:Post)<-[:replyOfComment|replyOfPost*0..]-(message:Comment|Post)-[:commentHasTag|postHasTag]->(:Tag)-[:hasType]->(:Tagclass {name: $tagClass})
RETURN
  forum.ID AS forumId,
  forum.title,
  forum.creationDate,
  person.ID AS personId,
  count(DISTINCT message) AS messageCount
ORDER BY
  messageCount DESC,
  forum.ID ASC
LIMIT 20
