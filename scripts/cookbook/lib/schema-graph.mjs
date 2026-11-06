// Data-model diagrams (ADR-0135). A ```mermaid flowchart whose nodes are all
// circles, `((…))`, is a recipe's graph model. GitHub draws it with mermaid;
// diagrams/render.mjs draws it for the website the way Neo4j's graph tools
// (Bloom, NVL) draw a graph: each label a coloured circle holding its
// properties, each relationship type along its arrow, laid out by force.
// This module reads the model from mermaid's parse of the block; the caller
// measures text. The work is split by concern:
//
//   schema-text.mjs      fonts; text fitted into circles and captions
//   schema-geometry.mjs  relationship curves, overlap tests
//   schema-layout.mjs    the force layout and how a drawing is scored
//   schema-draw.mjs      the SVG, per theme
//
// A node's text is `<b>Label</b><br/>prop · prop<br/>prop`: the first line
// names the label, the rest list its properties. A relationship's text is
// `<b>TYPE</b><br/>prop · prop`. Class `l1`…`l12` gives a label its colour
// from the palette; a dotted edge (`-. … .->`) is drawn dashed.

import { NEUTRAL, PALETTE } from './palette.mjs';

export { drawSchema } from './schema-draw.mjs';
export { layoutSchema } from './schema-layout.mjs';
export { FONTS, fontSpec } from './schema-text.mjs';

const ENTITIES = { amp: '&', lt: '<', gt: '>', quot: '"', '#39': "'", nbsp: ' ' };

/** The lines of a mermaid label, as plain text. */
function textLines(html) {
  return html
    .split(/<br\s*\/?>/i)
    .map((l) => l.replace(/<[^>]*>/g, '').replace(/&(amp|lt|gt|quot|#39|nbsp);/g, (_, e) => ENTITIES[e]).trim())
    .filter(Boolean);
}

/**
 * Whether a parsed flowchart is a data model: at least one node, every node
 * a circle, no subgraphs. `parsed` is what diagrams/render.mjs reads from
 * mermaid: {direction, subgraphs, vertices: [{id, text, type, classes}],
 * edges: [{start, end, text, stroke, type}]}.
 */
export function isSchema(parsed) {
  return parsed.subgraphs === 0 && parsed.vertices.length > 0 && parsed.vertices.every((v) => v.type === 'circle');
}

/** The graph a parsed data-model flowchart draws: {direction, nodes, edges}. */
export function schemaModel(parsed) {
  const nodes = parsed.vertices.map((v) => {
    const [label = v.id, ...rest] = textLines(v.text ?? '');
    const cls = v.classes.find((c) => /^l\d+$/.test(c));
    const color = cls ? PALETTE[Number(cls.slice(1)) - 1] : NEUTRAL;
    if (!color) throw new Error(`node ${v.id} has class ${cls}; label classes run l1…l${PALETTE.length}`);
    const props = rest.flatMap((l) => l.split(' · ')).map((p) => p.trim()).filter(Boolean);
    return { id: v.id, label, props, color };
  });
  const edges = parsed.edges.map((e) => {
    const [type = '', ...rest] = textLines(e.text ?? '');
    return {
      source: e.start,
      target: e.end,
      type,
      props: rest.join(' · '),
      dashed: e.stroke === 'dotted',
      arrow: e.type !== 'arrow_open',
    };
  });
  return { direction: parsed.direction, nodes, edges };
}

/**
 * The colour of each label a data model (`schemaModel`) draws, by the label
 * as the diagram names it: what the result graph view colours that label
 * with. Throws when two nodes give one label different colours.
 */
export function schemaLabels(model) {
  const labels = new Map();
  for (const n of model.nodes) {
    const seen = labels.get(n.label);
    if (seen && seen !== n.color) throw new Error(`label ${n.label} has two colours, ${seen} and ${n.color}`);
    labels.set(n.label, n.color);
  }
  return Object.fromEntries(labels);
}
