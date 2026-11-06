// q25: Does Amit Singh know anyone who studied at MIT_School_of_Engineering?
MATCH (amit:Person)-[:knows]->(p2:Person)-[:studyAt]->(o:Organisation)
WHERE amit.firstName = "Amit" AND amit.lastName = "Singh"
  AND o.name = "MIT_School_of_Engineering"
RETURN COUNT(DISTINCT p2.ID) > 0 AS knows_someone
