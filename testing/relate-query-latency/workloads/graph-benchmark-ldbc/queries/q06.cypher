// q6: IDs of employees who work at Nova_Air and whose last name contains Bravo.
MATCH (p:Person)-[:workAt]->(o:Organisation)
WHERE o.name = "Nova_Air" AND p.lastName CONTAINS "Bravo"
RETURN p.ID
