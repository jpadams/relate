export interface Sample {
  label: string;
  query: string;
}

export const SAMPLES: Sample[] = [
  {
    label: 'Colleagues at Neo4j',
    query: `MATCH (p:Person)-[r:WORKS_AT]->(c:Company {name: 'Neo4j'})
RETURN p, r, c`,
  },
  {
    label: 'Friends of friends (2–3 hops)',
    query: `MATCH path = (a:Person {name: 'Alice'})-[:KNOWS*2..3]->(other:Person)
WHERE other.name <> 'Alice'
RETURN path
LIMIT 10`,
  },
  {
    label: 'People ordered by age',
    query: `MATCH (p:Person)
RETURN p.name AS name, p.age AS age
ORDER BY age DESC
LIMIT 10`,
  },
  {
    label: 'Friend count per person',
    query: `MATCH (p:Person)-[:KNOWS]-(f:Person)
RETURN p.name AS person, count(DISTINCT f) AS friends
ORDER BY friends DESC, person`,
  },

  {
    label: 'Shared university alumni',
    query: `MATCH (p1:Person)-[:STUDIED_AT]->(u:University)<-[:STUDIED_AT]-(p2:Person)
WHERE p1.name < p2.name
RETURN u.name AS university, p1.name AS a, p2.name AS b
ORDER BY university, a, b`,
  },
  {
    label: 'Posts by tag (aggregation)',
    query: `MATCH (t:Tag)<-[:HAS_TAG]-(post:Post)
RETURN t.name AS tag, count(post) AS posts, collect(post.content) AS titles
ORDER BY posts DESC, tag`,
  },
  {
    label: 'People with no likes',
    query: `MATCH (p:Person)
OPTIONAL MATCH (p)-[l:LIKES]->(:Post)
WITH p, count(l) AS likes
WHERE likes = 0
RETURN p.name AS name, likes`,
  },
  {
    label: 'Cross-city colleagues',
    query: `MATCH (p1:Person)-[:LIVES_IN]->(c1:City),
      (p1)-[:WORKS_AT]->(co:Company)<-[:WORKS_AT]-(p2:Person),
      (p2)-[:LIVES_IN]->(c2:City)
WHERE c1 <> c2 AND p1.name < p2.name
RETURN p1.name AS a, c1.name AS a_city,
       p2.name AS b, c2.name AS b_city,
       co.name AS company`,
  },
  {
    label: 'Top authors by likes received',
    query: `MATCH (author:Person)-[:AUTHORED]->(post:Post)<-[:LIKES]-(fan:Person)
WITH author, count(fan) AS likes_received
RETURN author.name AS author, likes_received
ORDER BY likes_received DESC
LIMIT 5`,
  }
];
