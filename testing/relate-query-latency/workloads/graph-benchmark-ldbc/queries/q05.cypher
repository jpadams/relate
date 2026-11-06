// q5: Full names of persons with last name Choi who are members of forums containing John Brown.
MATCH (f:Forum)-[:hasMember]->(p:Person)
WHERE f.title CONTAINS "John Brown"
  AND p.lastName CONTAINS "Choi"
RETURN DISTINCT p.firstName, p.lastName
LIMIT 10
