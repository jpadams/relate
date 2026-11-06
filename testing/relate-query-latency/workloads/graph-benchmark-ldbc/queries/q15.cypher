// q15: Total number of forums moderated by employees of Air_Tanzania.
MATCH (f:Forum)-[:hasModerator]->(p:Person)-[:workAt]->(o:Organisation)
WHERE o.name = "Air_Tanzania"
RETURN COUNT(DISTINCT f.ID) AS num_forums
