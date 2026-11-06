// Relate wire envelope: relate/relate-model/src/wire.rs
// Values are JSON with __type tags for graph entities.

export type CLValue =
  | null
  | boolean
  | number
  | string
  | CLValue[]
  | CLNode
  | CLRelationship
  | CLPath
  | { [k: string]: CLValue };

export interface CLNode {
  id: string | number;
  elementId?: string;
  labels: string[];
  properties: Record<string, CLValue>;
}

export interface CLRelationship {
  id: string | number;
  elementId?: string;
  type: string;
  start_node: string | number;
  end_node: string | number;
  properties: Record<string, CLValue>;
}

export interface CLPath {
  nodes: CLNode[];
  relationships: CLRelationship[];
  // Relate emits `segments` too; keep loose.
  [k: string]: unknown;
}

export interface QueryStats {
  nodes_created?: number;
  nodes_deleted?: number;
  relationships_created?: number;
  relationships_deleted?: number;
  properties_set?: number;
  labels_added?: number;
  labels_removed?: number;
  indexes_added?: number;
  indexes_removed?: number;
  constraints_added?: number;
  constraints_removed?: number;
  [k: string]: number | undefined;
}

export interface WireResult {
  columns: string[];
  // Each row is a { columnName: value } object, per relate-model/src/wire.rs.
  rows: Array<Record<string, CLValue>>;
  stats: QueryStats;
  error: string | null;
}

// Relate's wire JSON tags graph entities by shape, not by a __type field.
export function isNode(v: unknown): v is CLNode {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return false;
  const o = v as Record<string, unknown>;
  return 'id' in o && Array.isArray(o.labels) && typeof o.properties === 'object';
}

export function isRelationship(v: unknown): v is CLRelationship {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return false;
  const o = v as Record<string, unknown>;
  return (
    'id' in o &&
    typeof o.type === 'string' &&
    'start_node' in o &&
    'end_node' in o &&
    typeof o.properties === 'object'
  );
}

export function isPath(v: unknown): v is CLPath {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return false;
  const o = v as Record<string, unknown>;
  return Array.isArray(o.nodes) && Array.isArray(o.relationships);
}
