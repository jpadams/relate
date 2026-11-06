import type { NeoNode, NeoRel, PortableProperty } from '@neo4j-ndl/react-graph';
import type { CLNode, CLPath, CLRelationship, CLValue, WireResult } from './wire';
import { isNode, isPath, isRelationship } from './wire';

export interface NeedleGraph {
  nodes: NeoNode[];
  rels: NeoRel[];
  limitHit: boolean;
}

const nodeId = (n: CLNode) => String(n.elementId ?? n.id);
const relId = (r: CLRelationship) => String(r.elementId ?? r.id);

function forEachEntity(
  result: WireResult,
  onNode: (n: CLNode) => void,
  onRel: (r: CLRelationship) => void,
): void {
  const visit = (v: CLValue): void => {
    if (v === null || v === undefined) return;
    if (Array.isArray(v)) return v.forEach(visit);
    if (typeof v !== 'object') return;
    if (isNode(v)) return onNode(v);
    if (isRelationship(v)) return onRel(v);
    if (isPath(v)) {
      (v as CLPath).nodes.forEach(onNode);
      (v as CLPath).relationships.forEach(onRel);
      return;
    }
    Object.values(v as Record<string, CLValue>).forEach(visit);
  };
  for (const row of result.rows) Object.values(row).forEach(visit);
}

export function resultHasGraph(result: WireResult): boolean {
  let found = false;
  forEachEntity(
    result,
    () => {
      found = true;
    },
    () => {
      found = true;
    },
  );
  return found;
}

export function extractNeedleGraph(
  result: WireResult,
  { nodeLimit }: { nodeLimit?: number } = {},
): NeedleGraph {
  const nodes = new Map<string, CLNode>();
  const rels = new Map<string, CLRelationship>();
  let limitHit = false;

  forEachEntity(
    result,
    (n) => {
      if (limitHit) return;
      const id = nodeId(n);
      if (!nodes.has(id)) nodes.set(id, n);
      if (typeof nodeLimit === 'number' && nodes.size >= nodeLimit) limitHit = true;
    },
    (r) => {
      const id = relId(r);
      if (!rels.has(id)) rels.set(id, r);
    },
  );

  const nodeIds = new Set(nodes.keys());
  const outNodes: NeoNode[] = Array.from(nodes.values()).map((n) => ({
    id: nodeId(n),
    labels: n.labels,
    properties: toPortableProperties(n.properties),
  }));
  const outRels: NeoRel[] = [];
  for (const r of rels.values()) {
    const from = String(r.start_node);
    const to = String(r.end_node);
    if (!nodeIds.has(from) || !nodeIds.has(to)) continue;
    outRels.push({
      id: relId(r),
      from,
      to,
      type: r.type,
      properties: toPortableProperties(r.properties),
    });
  }
  return { nodes: outNodes, rels: outRels, limitHit };
}

function toPortableProperties(p: Record<string, CLValue>): Record<string, PortableProperty> {
  const out: Record<string, PortableProperty> = {};
  for (const [k, v] of Object.entries(p)) out[k] = toPortable(v);
  return out;
}

function toPortable(v: CLValue): PortableProperty {
  if (v === null) return { stringified: 'null', type: 'null' };
  if (typeof v === 'boolean') return { stringified: String(v), type: 'boolean' };
  if (typeof v === 'number')
    return { stringified: String(v), type: Number.isInteger(v) ? 'integer' : 'float' };
  if (typeof v === 'string') return { stringified: JSON.stringify(v), type: 'string' };
  if (Array.isArray(v)) return { stringified: JSON.stringify(v), type: 'list' };
  return { stringified: JSON.stringify(v), type: 'map' };
}
