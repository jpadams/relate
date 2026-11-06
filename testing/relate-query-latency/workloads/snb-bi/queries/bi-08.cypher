// bi-08: Central person for a tag
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-8.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (22 rows, ~0.34 s — 2026-11-02 landing run)
MATCH (tag:Tag {name: $tag})
// score
OPTIONAL MATCH (tag)<-[interest:hasInterest]-(person:Person)
WITH tag, collect(person) AS interestedPersons
OPTIONAL MATCH (tag)<-[:commentHasTag|postHasTag]-(message:Comment|Post)-[:commentHasCreator|postHasCreator]->(person:Person)
         WHERE $startDate < datetime(message.creationDate)
           AND datetime(message.creationDate) < $endDate
WITH tag, interestedPersons, interestedPersons + collect(person) AS persons
UNWIND persons AS person
WITH DISTINCT tag, person
WITH
  tag,
  person,
  100 * size([(tag)<-[interest:hasInterest]-(person) | interest])
    + size([(tag)<-[:commentHasTag|postHasTag]-(message:Comment|Post)-[:commentHasCreator|postHasCreator]->(person)
             WHERE $startDate < datetime(message.creationDate) AND datetime(message.creationDate) < $endDate | message])
  AS score
OPTIONAL MATCH (person)-[:knows]-(friend)
WITH
  person,
  score,
  100 * size([(tag)<-[interest:hasInterest]-(friend) | interest])
    + size([(tag)<-[:commentHasTag|postHasTag]-(message:Comment|Post)-[:commentHasCreator|postHasCreator]->(friend)
             WHERE $startDate < datetime(message.creationDate) AND datetime(message.creationDate) < $endDate | message])
  AS friendScore
RETURN
  person.ID AS personId,
  score,
  sum(friendScore) AS friendsScore
ORDER BY
  score + friendsScore DESC,
  person.ID ASC
LIMIT 100
