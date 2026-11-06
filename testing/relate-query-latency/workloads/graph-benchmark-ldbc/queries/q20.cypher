// q20: Number of comments containing World War II with length > 1000.
MATCH (c:Comment)
WHERE c.content CONTAINS "World War II" AND c.length > 1000
RETURN COUNT(c.ID) AS long_comment_count
