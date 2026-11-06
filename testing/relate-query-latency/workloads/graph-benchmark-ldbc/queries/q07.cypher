// q7: Places where person 1786706544494 commented on posts tagged Jamaica.
MATCH (p:Person {ID: 1786706544494})<-[:commentHasCreator]-(c:Comment)
      -[:replyOfPost]->(post:Post)-[:postHasTag]->(t:Tag),
      (c)-[:commentIsLocatedIn]->(place:Place)
WHERE t.name = "Jamaica"
RETURN DISTINCT place.name
