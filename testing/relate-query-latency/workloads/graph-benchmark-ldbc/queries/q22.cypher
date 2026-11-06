// q22: Did anyone who works at Linxair create a comment that replied to a post?
MATCH (p:Person)-[:workAt]->(o:Organisation),
      (c:Comment)-[:replyOfPost]->(post:Post),
      (c)-[:commentHasCreator]->(p)
WHERE o.name = "Linxair"
RETURN COUNT(DISTINCT c.ID) > 0 AS has_reply_comment
