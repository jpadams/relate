// q30: Are there comments replying to posts created by the same person?
MATCH (c:Comment)-[:commentHasCreator]->(creator:Person),
      (c)-[:replyOfPost]->(post:Post)-[:postHasCreator]->(creator)
RETURN COUNT(DISTINCT c.ID) > 0 AS has_self_reply
