// q18: People studying at The_Oxford_Educational_Institutions with interest in William_Shakespeare.
MATCH (p:Person)-[:studyAt]->(o:Organisation), (p)-[:hasInterest]->(t:Tag)
WHERE o.name = "The_Oxford_Educational_Institutions"
  AND t.name = "William_Shakespeare"
RETURN COUNT(DISTINCT p.ID) AS num_p
