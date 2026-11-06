// q2: IDs of posts by Lei Zhang whose content contains Zulu.
MATCH (p:Person)<-[:postHasCreator]-(post:Post)
WHERE p.firstName = "Lei" AND p.lastName = "Zhang"
  AND post.content CONTAINS "Zulu"
RETURN post.ID
