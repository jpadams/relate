// bi-16: Fake news detection
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-16.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// translation note: graph-benchmark-ldbc creationDate is a datetime-shaped ISO string, so date() of it needs the mapping's datetime() wrap first — date(datetime(message.creationDate)); the param is a datetime value, date() of which converts directly (upstream's param is a date)
// status: ok at SF0.1 (2 rows, ~1.7 s — 2026-11-02 landing run; params substituted same-kind, see README)
UNWIND [
    {letter: 'A', tag: $tagA, date: $dateA},
    {letter: 'B', tag: $tagB, date: $dateB}
  ] AS param
WITH param.letter AS paramLetter, param.tag AS paramTagX, param.date AS paramDateX
CALL {
  WITH paramTagX, paramDateX
  MATCH (person1:Person)<-[:commentHasCreator|postHasCreator]-(message1:Comment|Post)-[:commentHasTag|postHasTag]->(tag:Tag {name: paramTagX})
  WHERE date(datetime(message1.creationDate)) = date(paramDateX)
  // filter out Persons with more than $maxKnowsLimit friends who created the same kind of Message
  OPTIONAL MATCH (person1)-[:knows]-(person2:Person)<-[:commentHasCreator|postHasCreator]-(message2:Comment|Post)-[:commentHasTag|postHasTag]->(tag)
  WHERE date(datetime(message2.creationDate)) = date(paramDateX)
  WITH person1, count(DISTINCT message1) AS cm, count(DISTINCT person2) AS cp2
  WHERE cp2 <= $maxKnowsLimit
  RETURN person1, cm
}
WITH person1, collect({letter: paramLetter, messageCount: cm}) AS results
WHERE size(results) = 2
RETURN
  person1.ID AS personId,
  [r IN results WHERE r.letter = 'A' | r.messageCount][0] AS messageCountA,
  [r IN results WHERE r.letter = 'B' | r.messageCount][0] AS messageCountB
ORDER BY messageCountA + messageCountB DESC, person1.ID ASC
LIMIT 20
