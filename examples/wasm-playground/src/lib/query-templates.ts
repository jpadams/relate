export const templates = {
  allNodes: () => 'MATCH (n) RETURN n LIMIT 25',
  nodesByLabel: (label: string) => `MATCH (n:${label}) RETURN n LIMIT 25`,
  allRels: () => 'MATCH (a)-[r]->(b) RETURN a, r, b LIMIT 25',
  relsByType: (type: string) => `MATCH (a)-[r:${type}]->(b) RETURN a, r, b LIMIT 25`,
  propertyValues: (key: string) =>
    [
      `MATCH (n)`,
      `WHERE n.${key} IS NOT NULL`,
      `RETURN DISTINCT "node" AS entity, n.${key} AS ${key} LIMIT 25`,
      `UNION ALL`,
      `MATCH ()-[r]-()`,
      `WHERE r.${key} IS NOT NULL`,
      `RETURN DISTINCT "relationship" AS entity, r.${key} AS ${key} LIMIT 25`,
    ].join('\n'),
  labeled: (label: string, query: string) => `// ${label}\n${query}`,
};
