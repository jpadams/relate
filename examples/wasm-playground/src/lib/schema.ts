import { engine } from './engine';

export interface DbSchema {
  nodeCount: number;
  relCount: number;
  labels: string[];
  relTypes: string[];
  propertyKeys: string[];
}

async function scalar(query: string, column: string): Promise<number> {
  const r = await engine.execute(query, null);
  if (r.error || r.rows.length === 0) return 0;
  const v = r.rows[0][column];
  return typeof v === 'number' ? v : 0;
}

async function stringColumn(query: string, column: string): Promise<string[]> {
  const r = await engine.execute(query, null);
  if (r.error) return [];
  return r.rows.map((row) => String(row[column] ?? '')).filter(Boolean).sort();
}

export async function loadSchema(): Promise<DbSchema> {
  const [nodeCount, relCount, labels, relTypes, propertyKeys] = await Promise.all([
    scalar('MATCH (n) RETURN count(n) AS c', 'c'),
    scalar('MATCH ()-[r]->() RETURN count(r) AS c', 'c'),
    stringColumn('CALL db.labels() YIELD label RETURN label', 'label'),
    stringColumn('CALL db.relationshipTypes() YIELD relationshipType RETURN relationshipType', 'relationshipType'),
    stringColumn('CALL db.propertyKeys() YIELD propertyKey RETURN propertyKey', 'propertyKey'),
  ]);
  return { nodeCount, relCount, labels, relTypes, propertyKeys };
}
