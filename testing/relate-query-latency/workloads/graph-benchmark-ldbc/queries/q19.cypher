// q19: Place with most comments whose tag contains Copernicus.
MATCH (c:Comment)-[:commentHasTag]->(t:Tag), (c)-[:commentIsLocatedIn]->(l:Place)
WHERE t.name CONTAINS "Copernicus"
RETURN l.name, COUNT(c.ID) AS comment_count
ORDER BY comment_count DESC
LIMIT 1
