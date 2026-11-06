// bi-13: Zombies in a country
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-13.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (1 row, ~5 ms — 2026-11-02 landing run)
MATCH (country:Place {name: $country})<-[:isPartOf]-(:Place)<-[:personIsLocatedIn]-(zombie:Person)
WHERE datetime(zombie.creationDate) < $endDate
WITH country, zombie
OPTIONAL MATCH (zombie)<-[:commentHasCreator|postHasCreator]-(message:Comment|Post)
WHERE datetime(message.creationDate) < $endDate
WITH
  country,
  zombie,
  count(message) AS messageCount
WITH
  country,
  zombie,
  12 * ($endDate.year  - datetime(zombie.creationDate).year )
     + ($endDate.month - datetime(zombie.creationDate).month)
     + 1 AS months,
  messageCount
WHERE messageCount / months < 1
WITH
  country,
  collect(zombie) AS zombies
UNWIND zombies AS zombie
OPTIONAL MATCH
  (zombie)<-[:commentHasCreator|postHasCreator]-(message:Comment|Post)<-[:likeComment|likePost]-(likerZombie:Person)
WHERE likerZombie IN zombies
WITH
  zombie,
  count(likerZombie) AS zombieLikeCount
OPTIONAL MATCH
  (zombie)<-[:commentHasCreator|postHasCreator]-(message:Comment|Post)<-[:likeComment|likePost]-(likerPerson:Person)
WHERE datetime(likerPerson.creationDate) < $endDate
WITH
  zombie,
  zombieLikeCount,
  count(likerPerson) AS totalLikeCount
RETURN
  zombie.ID AS zombieId,
  zombieLikeCount,
  totalLikeCount,
  CASE totalLikeCount
    WHEN 0 THEN 0.0
    ELSE zombieLikeCount / toFloat(totalLikeCount)
  END AS zombieScore
ORDER BY
  zombieScore DESC,
  zombie.ID ASC
LIMIT 100
