// q10: Unique IDs of persons who commented on posts tagged Cate_Blanchett.
MATCH (c:Comment)-[:replyOfPost]->(post:Post)-[:postHasTag]->(t:Tag),
      (c)-[:commentHasCreator]->(p:Person)
WHERE t.name = "Cate_Blanchett"
RETURN DISTINCT p.ID
