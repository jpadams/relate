// q13: Total number of persons who liked comments created by Rafael Alonso.
MATCH (p:Person)<-[:commentHasCreator]-(c:Comment)<-[:likeComment]-(p2:Person)
WHERE p.firstName = "Rafael" AND p.lastName = "Alonso"
RETURN COUNT(DISTINCT p2.ID) AS num_persons
