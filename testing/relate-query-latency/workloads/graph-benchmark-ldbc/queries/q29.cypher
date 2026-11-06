// q29: Has Justine Fenter written a post using Safari?
MATCH (p:Person)<-[:postHasCreator]-(post:Post)
WHERE p.firstName = "Justine" AND p.lastName = "Fenter"
  AND post.browserUsed CONTAINS "Safari"
RETURN COUNT(post.ID) > 0 AS has_written_post_with_safari
