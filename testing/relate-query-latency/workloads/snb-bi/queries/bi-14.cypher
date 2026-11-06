// bi-14: International dialog
// source: ldbc/ldbc_snb_bi @ 47dd38b40844ecdb0e42e5a610c369535304786d neo4j/queries/bi-14.cypher (Apache-2.0), mechanically translated to the graph-benchmark-ldbc vocabulary (see params-sf0.1.json _source.mapping)
// status: ok at SF0.1 (5 rows, ~0.1 s — 2026-11-02 landing run)
MATCH
  (country1:Place {name: $country1})<-[:isPartOf]-(city1:Place)<-[:personIsLocatedIn]-(person1:Person),
  (country2:Place {name: $country2})<-[:isPartOf]-(city2:Place)<-[:personIsLocatedIn]-(person2:Person),
  (person1)-[:knows]-(person2)
WITH person1, person2, city1, 0 AS score
// case 1
OPTIONAL MATCH (person1)<-[:commentHasCreator]-(c:Comment)-[:replyOfComment|replyOfPost]->(:Comment|Post)-[:commentHasCreator|postHasCreator]->(person2)
WITH DISTINCT person1, person2, city1, score + (CASE WHEN c IS NULL THEN 0 ELSE  4 END) AS score
// case 2
OPTIONAL MATCH (person1)<-[:commentHasCreator|postHasCreator]-(m:Comment|Post)<-[:replyOfComment|replyOfPost]-(:Comment)-[:commentHasCreator]->(person2)
WITH DISTINCT person1, person2, city1, score + (CASE WHEN m IS NULL THEN 0 ELSE  1 END) AS score
// case 3
OPTIONAL MATCH (person1)-[:likeComment|likePost]->(m:Comment|Post)-[:commentHasCreator|postHasCreator]->(person2)
WITH DISTINCT person1, person2, city1, score + (CASE WHEN m IS NULL THEN 0 ELSE 10 END) AS score
// case 4
OPTIONAL MATCH (person1)<-[:commentHasCreator|postHasCreator]-(m:Comment|Post)<-[:likeComment|likePost]-(person2)
WITH DISTINCT person1, person2, city1, score + (CASE WHEN m IS NULL THEN 0 ELSE  1 END) AS score
// preorder
ORDER BY
  city1.name ASC,
  score DESC,
  person1.ID ASC,
  person2.ID ASC
WITH city1, collect({score: score, person1Id: person1.ID, person2Id: person2.ID})[0] AS top
RETURN
  top.person1Id,
  top.person2Id,
  city1.name,
  top.score
ORDER BY
  top.score DESC,
  top.person1Id ASC,
  top.person2Id ASC
LIMIT 100
