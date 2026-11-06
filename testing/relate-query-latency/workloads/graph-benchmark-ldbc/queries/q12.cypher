// q12: Total number of comments with non-null content created by people in Berlin.
MATCH (c:Comment)-[:commentHasCreator]->(p:Person)-[:personIsLocatedIn]->(l:Place)
WHERE c.content IS NOT NULL AND l.name = "Berlin"
RETURN COUNT(DISTINCT c.ID) AS num_comments
