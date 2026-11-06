// q17: Most common interest tag among people who studied at Indian_Institute_of_Science.
MATCH (p:Person)-[:studyAt]->(o:Organisation), (p)-[:hasInterest]->(t:Tag)
WHERE o.name = "Indian_Institute_of_Science"
RETURN t.name, COUNT(*) AS tag_count
ORDER BY tag_count DESC
LIMIT 1
