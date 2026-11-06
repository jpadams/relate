import assert from 'node:assert/strict';
import { test } from 'node:test';

import { inertSvg, prepareSvg } from './diagrams.mjs';
import { NEUTRAL, outline, PALETTE, PALETTE_BY_NAME } from './palette.mjs';
import { bezierAt, toCubic } from './schema-geometry.mjs';
import { drawSchema, FONTS, isSchema, layoutSchema, schemaLabels, schemaModel } from './schema-graph.mjs';
import { LINE } from './schema-text.mjs';

// Every character 0.55 em wide.
const measure = (text, font) => text.length * font.size * 0.55;

const vertex = (id, text, classes = [], type = 'circle') => ({ id, text, type, classes });
const edge = (start, end, text, stroke = 'normal', type = 'arrow_point') => ({ start, end, text, stroke, type });

// What mermaid's parser gives for a data model with a loop and two
// relationships between the same pair.
const PARSED = {
  direction: 'LR',
  subgraphs: 0,
  vertices: [
    vertex('Agent', '<b>Agent</b><br>id', ['l1']),
    vertex('Place', '<b>Place</b><br>name · kind<br>loc', ['l2']),
    vertex('City', '<b>City</b><br>name &amp; code'),
  ],
  edges: [
    edge('Agent', 'Place', '<b>VISITED</b><br>ts · seq'),
    edge('Agent', 'Place', '<b>LIKES</b>'),
    edge('Place', 'City', '<b>IN</b>', 'normal', 'arrow_open'),
    edge('Place', 'Place', '<b>NEAR</b><br>km', 'dotted'),
  ],
};

test('isSchema accepts flowcharts whose nodes are all circles', () => {
  assert.equal(isSchema(PARSED), true);
  assert.equal(isSchema({ ...PARSED, subgraphs: 1 }), false);
  assert.equal(isSchema({ ...PARSED, vertices: [...PARSED.vertices, vertex('Q', 'Q', [], 'round')] }), false);
  assert.equal(isSchema({ ...PARSED, vertices: [] }), false);
});

test('schemaModel reads labels, properties, colours, and relationships', () => {
  const model = schemaModel(PARSED);
  assert.deepEqual(
    model.nodes.map(({ id, label, props, color }) => ({ id, label, props, color })),
    [
      { id: 'Agent', label: 'Agent', props: ['id'], color: PALETTE[0] },
      { id: 'Place', label: 'Place', props: ['name', 'kind', 'loc'], color: PALETTE[1] },
      { id: 'City', label: 'City', props: ['name & code'], color: NEUTRAL },
    ],
  );
  assert.deepEqual(model.edges[0], { source: 'Agent', target: 'Place', type: 'VISITED', props: 'ts · seq', dashed: false, arrow: true });
  assert.equal(model.edges[2].arrow, false);
  assert.equal(model.edges[3].dashed, true);
  assert.throws(() => schemaModel({ ...PARSED, vertices: [vertex('A', 'A', ['l13'])] }), /label classes run l1…l12/);
});

test('schemaLabels maps each label, as the diagram names it, to its colour', () => {
  assert.deepEqual(schemaLabels(schemaModel(PARSED)), { Agent: PALETTE[0], Place: PALETTE[1], City: NEUTRAL });
  // A label split over two lines in the drawing is still one label.
  const split = { ...PARSED, vertices: [vertex('D', '<b>Doc:ADR</b><br>id', ['l3']), vertex('E', '<b>Doc:ADR</b>', ['l3'])], edges: [] };
  assert.deepEqual(schemaLabels(schemaModel(split)), { 'Doc:ADR': PALETTE[2] });
  const clash = { ...split, vertices: [split.vertices[0], vertex('E', '<b>Doc:ADR</b>', ['l4'])] };
  assert.throws(() => schemaLabels(schemaModel(clash)), /label Doc:ADR has two colours/);
});

test('the palette is named in order', () => {
  assert.deepEqual(PALETTE, Object.values(PALETTE_BY_NAME));
  assert.equal(PALETTE.length, 12);
  assert.equal(PALETTE_BY_NAME.blue, PALETTE[8]);
});

const close = (p, q) => Math.hypot(p.x - q.x, p.y - q.y) < 1e-9;

test('toCubic raises a line or a quadratic to the same curve', () => {
  const [a, c, b] = [{ x: 0, y: 0 }, { x: 30, y: 60 }, { x: 90, y: 0 }];
  const quad = toCubic([a, c, b]);
  assert.ok(close(quad.c1, { x: 20, y: 40 }) && close(quad.c2, { x: 50, y: 40 }));
  const line = toCubic([a, b]);
  assert.ok(close(line.c1, { x: 30, y: 0 }) && close(line.c2, { x: 60, y: 0 }));
  for (let t = 0; t <= 1; t += 0.125) {
    assert.ok(close(bezierAt([quad.start, quad.c1, quad.c2, quad.end], t), bezierAt([a, c, b], t)), `quadratic at ${t}`);
    assert.ok(close(bezierAt([line.start, line.c1, line.c2, line.end], t), { x: 90 * t, y: 0 }), `line at ${t}`);
  }
});

test('layoutSchema is deterministic and draws without flaws', () => {
  const layout = layoutSchema(schemaModel(PARSED), measure);
  const again = layoutSchema(schemaModel(PARSED), measure);
  assert.deepEqual(
    layout.nodes.map((n) => [n.x, n.y, n.r]),
    again.nodes.map((n) => [n.x, n.y, n.r]),
  );
  assert.deepEqual(layout.problems, []);

  // Each line of text fits inside its circle.
  for (const n of layout.nodes) {
    for (const l of n.lines) {
      assert.ok(Math.hypot(l.w / 2, Math.abs(l.y) + (FONTS[l.font].size * LINE) / 2) <= n.r, `${n.id}: ${l.text}`);
    }
  }
  // Every relationship is a cubic curve. Place IN City is straight: its
  // control points sit at the thirds of its line. The two relationships
  // between Agent and Place bend apart, off their chord. The loop leaves and
  // meets its node; the unlabelled arrow_open edge has no head.
  const [visited, likes, into, near] = layout.edges;
  const at = (g, k) => ({ x: g.start.x + (g.end.x - g.start.x) * k, y: g.start.y + (g.end.y - g.start.y) * k });
  const offChord = (g, p) => {
    const d = Math.hypot(g.end.x - g.start.x, g.end.y - g.start.y);
    return Math.abs((g.end.x - g.start.x) * (p.y - g.start.y) - (g.end.y - g.start.y) * (p.x - g.start.x)) / d;
  };
  assert.equal(into.bend, 0);
  assert.ok(close(into.geo.c1, at(into.geo, 1 / 3)) && close(into.geo.c2, at(into.geo, 2 / 3)));
  assert.equal(Math.sign(visited.bend), -Math.sign(likes.bend));
  for (const e of [visited, likes]) assert.ok(offChord(e.geo, e.geo.c1) > 5 && offChord(e.geo, e.geo.c2) > 5);
  const place = layout.nodes.find((n) => n.id === 'Place');
  for (const p of [near.geo.start, near.geo.end]) assert.ok(Math.abs(Math.hypot(p.x - place.x, p.y - place.y) - place.r) < 12);
  assert.ok(near.geo.head);
  assert.equal(into.geo.head, null);
  // Captions stay upright.
  for (const e of layout.edges) assert.ok(Math.abs(e.geo.caption.angle) <= 90);
});

test('a long label splits where a word starts when that saves room', () => {
  const parsed = { ...PARSED, vertices: [vertex('C', '<b>IndustryCategory</b><br>id · name', ['l3'])], edges: [] };
  const [node] = layoutSchema(schemaModel(parsed), measure).nodes;
  assert.deepEqual(
    node.lines.filter((l) => l.font === 'label').map((l) => l.text),
    ['Industry', 'Category'],
  );
});

test('drawSchema draws an inert SVG the page can size', () => {
  const layout = layoutSchema(schemaModel(PARSED), measure);
  const light = drawSchema(layout, { id: 'd-t-1-light', theme: 'light', title: 'Data model: <agents>' });
  const dark = drawSchema(layout, { id: 'd-t-1-dark', theme: 'dark', title: 'Data model: <agents>' });
  for (const svg of [light, dark]) {
    inertSvg(svg, 'drawing');
    assert.match(prepareSvg(svg), /^<svg [^>]*style="max-width: \d+px; --natural-width: \d+px;"/);
    assert.equal(svg.match(/<circle /g).length, 3);
  }
  assert.match(light, /<title id="d-t-1-light-title">Data model: &lt;agents&gt;<\/title>/);
  assert.match(light, /<desc id="d-t-1-light-desc">Agent VISITED Place; Agent LIKES Place; Place IN City; Place NEAR Place<\/desc>/);
  assert.match(light, /class="rel dash"/);
  // One path shape for every relationship: a cubic.
  assert.equal(light.match(/class="rel[^"]*" d="M[\d.,-]+ C[\d.,-]+ [\d.,-]+ [\d.,-]+"/g).length, 4);
  // Same geometry, different colours.
  const shape = (svg) => svg.replace(/<style>.*<\/style>/, '').replace(/d-t-1-(light|dark)/g, '');
  assert.equal(shape(light), shape(dark));
  assert.notEqual(light, dark);
});

test('outline darkens each channel', () => {
  assert.equal(outline('#FFDF81'), '#C7AE65');
  assert.equal(outline('#000000'), '#000000');
});
