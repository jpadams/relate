// q27: Did any person from Toronto create a comment with tag Winston_Churchill?
MATCH (c:Comment)-[:commentHasCreator]->(p:Person),
      (p)-[:personIsLocatedIn]->(l:Place),
      (c)-[:commentHasTag]->(t:Tag)
WHERE l.name = "Toronto" AND t.name = "Winston_Churchill"
RETURN COUNT(DISTINCT c.ID) > 0 AS has_comment
