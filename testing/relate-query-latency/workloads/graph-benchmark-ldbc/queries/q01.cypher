// q1: Who are the names of people who live in Glasgow and are interested in Napoleon?
MATCH (p:Person)-[:personIsLocatedIn]->(pl:Place),
      (p)-[:hasInterest]->(t:Tag)
WHERE pl.name = "Glasgow" AND t.name = "Napoleon"
RETURN p.firstName, p.lastName
