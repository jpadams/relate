// q23: Is there a person with last name Gurung who is a moderator of a forum tagged Norah_Jones?
MATCH (p:Person)<-[:hasModerator]-(f:Forum)-[:forumHasTag]->(t:Tag)
WHERE t.name = "Norah_Jones" AND p.lastName = "Gurung"
RETURN COUNT(DISTINCT p.ID) > 0 AS has_moderator
