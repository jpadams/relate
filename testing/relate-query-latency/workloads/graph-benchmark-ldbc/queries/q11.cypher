// q11: Non-university organization with most employees.
MATCH (p:Person)-[:workAt]->(o:Organisation)
WHERE o.type <> "university"
RETURN COUNT(DISTINCT p.ID) AS num_e, o.name
ORDER BY num_e DESC
LIMIT 1
