// q24: Is there a person who lives in Paris and is interested in Cate_Blanchett?
MATCH (p:Person)-[:personIsLocatedIn]->(l:Place), (p)-[:hasInterest]->(t:Tag)
WHERE l.name = "Paris" AND t.name = "Cate_Blanchett"
RETURN COUNT(DISTINCT p.ID) > 0 AS has_person
