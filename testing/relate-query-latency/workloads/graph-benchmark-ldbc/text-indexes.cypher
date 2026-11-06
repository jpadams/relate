// The text index profile (`--indexes text`, ADR-0243): the upstream profile plus TEXT indexes on the three
// properties whose CONTAINS filters a TEXT index can serve: Forum.title (q5, q8), Person.lastName (q5, q6) and
// Tag.name (q19). Equality filters on those properties stay on the RANGE indexes of indexes.cypher.

// @step text-indexes
CREATE TEXT INDEX forum_title_text FOR (n:Forum) ON (n.title);
CREATE TEXT INDEX person_lastName_text FOR (n:Person) ON (n.lastName);
CREATE TEXT INDEX tag_name_text FOR (n:Tag) ON (n.name);
