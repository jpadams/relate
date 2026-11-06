// q21: Has Bill Moore liked the post with ID 1649268446863?
MATCH (p:Post)<-[:likePost]-(p2:Person)
WHERE p2.firstName = "Bill" AND p2.lastName = "Moore"
  AND p.ID = 1649268446863
RETURN COUNT(p.ID) > 0 AS liked
