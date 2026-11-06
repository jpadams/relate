// The upstream index profile (ADR-0243): upstream's schema keys every label on `ID`, plus the lookup indexes on the
// properties the 30 queries filter by equality or range. The first six lookup indexes are the secondary indexes
// upstream gave LadybugDB (ladybugdb/build_graph.py SECONDARY_INDEXES at c8eb87f); Organisation.name and
// Tagclass.name are added because an equality filter without a built index scans the label on every call. CONTAINS
// filters cannot use a RANGE index; the text profile (text-indexes.cypher) adds TEXT indexes for three of them.
// Cypher 25 removed `indexProvider` from OPTIONS (live-verified 22N04): these RANGE indexes carry
// no `OPTIONS` clause — range is the default kind and `range-1.0` its default provider, so the store is
// identical to the one the examples-repo harness builds.
//
// `// @step <name>` starts a group. The loader runs this file's first step (`id-indexes`) before the data, so every
// endpoint lookup during the load is a seek, and every later step after it.

// @step id-indexes
CREATE INDEX comment_ID FOR (n:Comment) ON (n.ID);
CREATE INDEX forum_ID FOR (n:Forum) ON (n.ID);
CREATE INDEX organisation_ID FOR (n:Organisation) ON (n.ID);
CREATE INDEX person_ID FOR (n:Person) ON (n.ID);
CREATE INDEX place_ID FOR (n:Place) ON (n.ID);
CREATE INDEX post_ID FOR (n:Post) ON (n.ID);
CREATE INDEX tag_ID FOR (n:Tag) ON (n.ID);
CREATE INDEX tagclass_ID FOR (n:Tagclass) ON (n.ID);

// @step lookup-indexes
CREATE INDEX place_name FOR (n:Place) ON (n.name);
CREATE INDEX tag_name FOR (n:Tag) ON (n.name);
CREATE INDEX person_firstName FOR (n:Person) ON (n.firstName);
CREATE INDEX person_lastName FOR (n:Person) ON (n.lastName);
CREATE INDEX person_birthday FOR (n:Person) ON (n.birthday);
CREATE INDEX comment_length FOR (n:Comment) ON (n.length);
CREATE INDEX organisation_name FOR (n:Organisation) ON (n.name);
CREATE INDEX tagclass_name FOR (n:Tagclass) ON (n.name);
