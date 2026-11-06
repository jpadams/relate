// q9: Persons with last name Johansson who know someone who studied in Tallinn.
MATCH (p:Person)-[:knows]->(p2:Person)-[:studyAt]->(o:Organisation)
      -[:organisationIsLocatedIn]->(l:Place)
WHERE l.name = "Tallinn" AND p.lastName = "Johansson"
RETURN p.ID, p.firstName, p.lastName
