// bi-04: Top message creators in a country
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-4.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: kept — times out at SF0.1 (cancelled at 60 s in the landing probe): the unbounded replyOfComment|replyOfPost*0.. walk inside the CALL over every top-forum post — the walk-budget class, LABS-660
MATCH (country:Place)<-[:isPartOf]-(:Place)<-[:personIsLocatedIn]-(person:Person)<-[:hasMember]-(forum:Forum)
WHERE datetime(forum.creationDate) > $date
WITH country, forum, count(person) AS numberOfMembers
ORDER BY numberOfMembers DESC, forum.ID ASC, country.ID
WITH DISTINCT forum AS topForum
LIMIT 100

WITH collect(topForum) AS topForums

CALL {
  WITH topForums
  UNWIND topForums AS topForum1
  MATCH (topForum1)-[:containerOf]->(post:Post)<-[:replyOfComment|replyOfPost*0..]-(message:Comment|Post)-[:commentHasCreator|postHasCreator]->(person:Person)<-[:hasMember]-(topForum2:Forum)
  WITH person, message, topForum2
  WHERE topForum2 IN topForums
  RETURN person, count(DISTINCT message) AS messageCount
UNION ALL
  // People who are members of top forums but have 0 messages are returned with messageCount 0.
  WITH topForums
  UNWIND topForums AS topForum1
  MATCH (person:Person)<-[:hasMember]-(topForum1:Forum)
  RETURN person, 0 AS messageCount
}
RETURN
  person.ID AS personId,
  person.firstName AS personFirstName,
  person.lastName AS personLastName,
  person.creationDate AS personCreationDate,
  sum(messageCount) AS messageCount
ORDER BY
  messageCount DESC,
  person.ID ASC
LIMIT 100
