// bi-17: Information propagation analysis
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-17.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// translation note: upstream spells the two HAS_MEMBER patterns `<-[:HAS_MEMBER]->`; the intended edge is forum->person (upstream bi-4 and this query's own NOT clause both spell it that way, and graph-benchmark-ldbc's edge is forum-[:hasMember]->person), so the translation is directional — upstream's both-arrowheads spelling matched nothing over the graph-benchmark-ldbc store
// status: kept — times out at SF0.1 (the no-watchdog landing probe was killed past 60 s, memory blow-up): the unbounded replyOfComment|replyOfPost*0.. walks per propagation anchor — the walk-budget class, LABS-660
MATCH
  (tag:Tag {name: $tag}),
  (person1:Person)<-[:commentHasCreator|postHasCreator]-(message1:Comment|Post)-[:replyOfComment|replyOfPost*0..]->(post1:Post)<-[:containerOf]-(forum1:Forum),
  (message1)-[:commentHasTag|postHasTag]->(tag),
// Two hasMember edges in the same MATCH clause force person2 <> person3
// (edge-isomorphic matching forbids sharing one edge).
  (forum1)-[:hasMember]->(person2:Person)<-[:commentHasCreator]-(comment:Comment)-[:commentHasTag]->(tag),
  (forum1)-[:hasMember]->(person3:Person)<-[:commentHasCreator|postHasCreator]-(message2:Comment|Post),
  (comment)-[:replyOfComment|replyOfPost]->(message2)-[:replyOfComment|replyOfPost*0..]->(post2:Post)<-[:containerOf]-(forum2:Forum)
// message2 = post2 would overlap the HAS_TAG edges; added in separate MATCH clauses.
MATCH (comment)-[:commentHasTag]->(tag)
MATCH (message2)-[:commentHasTag|postHasTag]->(tag)
WHERE forum1 <> forum2
  AND datetime(message2.creationDate) > datetime(message1.creationDate) + duration({hours: $delta})
  AND NOT (forum2)-[:hasMember]->(person1)
RETURN person1.ID AS personId, count(DISTINCT message2) AS messageCount
ORDER BY messageCount DESC, person1.ID ASC
LIMIT 10
