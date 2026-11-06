// q26: Are there any forums with tag Benjamin_Franklin that person 10995116287854 is a member of?
MATCH (f:Forum)-[:hasMember]->(p:Person), (f)-[:forumHasTag]->(t:Tag)
WHERE p.ID = 10995116287854 AND t.name = "Benjamin_Franklin"
RETURN COUNT(DISTINCT f.ID) > 0 AS has_forum
