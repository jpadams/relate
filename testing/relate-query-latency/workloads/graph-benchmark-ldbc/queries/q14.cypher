// q14: Number of forums with tags belonging to the Athlete tagclass.
MATCH (f:Forum)-[:forumHasTag]->(:Tag)-[:hasType]->(:Tagclass {name: "Athlete"})
RETURN COUNT(DISTINCT f.ID) AS num_forums
