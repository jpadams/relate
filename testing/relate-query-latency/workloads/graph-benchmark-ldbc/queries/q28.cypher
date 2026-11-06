// q28: Are there people in Manila interested in tags of type BritishRoyalty?
MATCH (t:Tag)-[:hasType]->(tc:Tagclass),
      (p:Person)-[:hasInterest]->(t),
      (p)-[:personIsLocatedIn]->(l:Place)
WHERE tc.name = "BritishRoyalty" AND l.name = "Manila"
RETURN COUNT(DISTINCT p.ID) > 0 AS has_people
