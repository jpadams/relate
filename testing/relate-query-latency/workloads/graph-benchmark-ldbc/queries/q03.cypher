// q3: Creator of post ID 962077547172 and where they studied.
MATCH (post:Post {ID: 962077547172})-[:postHasCreator]->(person:Person),
      (person)-[:studyAt]->(org:Organisation)
RETURN person.firstName, person.lastName, org.name
