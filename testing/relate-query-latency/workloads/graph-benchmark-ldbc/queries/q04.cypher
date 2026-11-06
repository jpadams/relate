// q4: Comment IDs by Alfredo Gomez with length > 100.
MATCH (p:Person)<-[:commentHasCreator]-(c:Comment)
WHERE p.firstName = "Alfredo"
  AND p.lastName = "Gomez"
  AND c.length > 100
RETURN c.ID
