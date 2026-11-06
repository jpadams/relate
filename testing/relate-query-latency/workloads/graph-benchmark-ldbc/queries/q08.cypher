// q8: Distinct IDs of persons born after 1990 who moderate forums containing Emilio Fernandez.
MATCH (p:Person)<-[:hasModerator]-(f:Forum)
WHERE date(p.birthday) > date("1990-01-01")
  AND f.title CONTAINS "Emilio Fernandez"
RETURN DISTINCT p.ID
