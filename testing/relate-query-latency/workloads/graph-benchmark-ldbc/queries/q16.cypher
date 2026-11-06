// q16: Number of posts containing Copernicus created by persons located in Mumbai.
MATCH (p:Person)-[:personIsLocatedIn]->(l:Place),
      (p)<-[:postHasCreator]-(post:Post)
WHERE l.name = "Mumbai" AND post.content CONTAINS "Copernicus"
RETURN COUNT(post.ID) AS num_posts
