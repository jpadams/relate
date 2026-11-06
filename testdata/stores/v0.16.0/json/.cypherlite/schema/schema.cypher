CREATE FULLTEXT INDEX `person_bio` FOR (n:`Person`) ON EACH [n.`bio`] OPTIONS {indexConfig: {`fulltext.analyzer`: 'standard-no-stop-words',`fulltext.eventually_consistent`: false}};
CREATE VECTOR INDEX `person_embedding` FOR (n:`Person`) ON (n.`embedding`) OPTIONS {indexConfig: {`vector.default_search_expansion_factor`: 3.0,`vector.dimensions`: 3,`vector.hnsw.ef_construction`: 100,`vector.hnsw.m`: 16,`vector.quantization.type`: 'BINARY',`vector.similarity_function`: 'COSINE'}};
CREATE RANGE INDEX `person_name` FOR (n:`Person`) ON (n.`name`);
CREATE CONSTRAINT `person_email_unique` FOR (n:`Person`) REQUIRE (n.`email`) IS UNIQUE;
CREATE CONSTRAINT `person_name_exists` FOR (n:`Person`) REQUIRE (n.`name`) IS NOT NULL;
