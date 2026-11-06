import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  CONNECTING,
  DrawnGraph,
  EXPAND_LIMIT,
  NEIGHBOURS,
  NODE_SIZE,
  caption,
  expandNode,
  formatValue,
  idOf,
  kindOf,
  labelColor,
  labelColors,
  nvlNode,
  nvlRel,
} from './graph-model.mjs';
import { fakeRun, node, rel } from './graph-fixtures.mjs';
import { NEUTRAL, PALETTE } from './palette.mjs';

test('labelColor takes the diagram colour of the label set, then of a label, then a stable palette pick', () => {
  const colors = labelColors('{"Version:Root": "#111111", "Version": "#222222"}');
  assert.equal(labelColor(['Root', 'Version'], colors), '#111111');
  assert.equal(labelColor(['Version', 'Deprecated'], colors), '#222222');
  assert.equal(labelColor([], colors), NEUTRAL);
  const picked = labelColor(['Other'], colors);
  assert.ok(PALETTE.includes(picked));
  assert.equal(labelColor(['Other'], labelColors('')), picked);
});

test('caption prefers naming properties, then any string, then the label', () => {
  assert.equal(caption(node(1, ['A'], { key: 'k', name: 'n' })), 'n');
  assert.equal(caption(node(1, ['A'], { id: 7 })), '7');
  assert.equal(caption(node(1, ['A'], { n: 1, s: 'text' })), 'text');
  assert.equal(caption(node(1, ['A'], { n: 1 })), 'A');
  assert.equal(caption(node(3, [], {})), '3');
});

test('formatValue shows strings as they are and cuts long values', () => {
  assert.equal(formatValue('a "b"'), 'a "b"');
  assert.equal(formatValue([1, 'x']), '[1,"x"]');
  assert.equal(formatValue('abcdef', 4), 'abc…');
});

test('NVL ids keep nodes and relationships apart and map back to engine ids', () => {
  const n = nvlNode(node(19, ['A'], { name: 'x' }), new Map([['A', '#123456']]));
  assert.deepEqual(n, { id: 'n19', caption: 'x', color: '#123456', size: NODE_SIZE });
  const r = nvlRel(rel(19, 19, 4, 'KNOWS'), '#999999');
  assert.deepEqual(r, { id: 'r19', from: 'n19', to: 'n4', type: 'KNOWS', caption: 'KNOWS', color: '#999999' });
  assert.equal(idOf(r.id), 19);
  assert.equal(kindOf(n.id), 'node');
  assert.equal(kindOf(r.id), 'relationship');
});

test('DrawnGraph adds only new elements, and relationships whose ends are both drawn', () => {
  const drawn = new DrawnGraph();
  const first = drawn.add({ nodes: [node(1), node(2)], rels: [rel(10, 1, 2), rel(11, 2, 3)] });
  assert.deepEqual(first.nodes.map((n) => n.id), [1, 2]);
  assert.deepEqual(first.rels.map((r) => r.id), [10]);
  const second = drawn.add({ nodes: [node(2), node(3)], rels: [rel(10, 1, 2), rel(11, 2, 3)] });
  assert.deepEqual(second.nodes.map((n) => n.id), [3]);
  assert.deepEqual(second.rels.map((r) => r.id), [11]);
  assert.equal(second.left, 0);
});

test('DrawnGraph.get finds a drawn element by its NVL id', () => {
  const drawn = new DrawnGraph();
  drawn.add({ nodes: [node(1), node(2)], rels: [rel(1, 1, 2)] });
  assert.equal(drawn.get('n1').labels[0], 'A');
  assert.equal(drawn.get('r1').type, 'R');
  assert.equal(drawn.get('n9'), undefined);
});

test('DrawnGraph stops at its limit and counts the nodes left out', () => {
  const drawn = new DrawnGraph();
  const added = drawn.add({ nodes: [node(1), node(2), node(3)], rels: [rel(10, 1, 2), rel(11, 1, 3)] }, 2);
  assert.deepEqual(added.nodes.map((n) => n.id), [1, 2]);
  assert.deepEqual(added.rels.map((r) => r.id), [10]);
  assert.equal(added.left, 1);
});

test('DrawnGraph.keepOnly drops what the kept graph does not draw', () => {
  const original = new DrawnGraph();
  original.add({ nodes: [node(1), node(2)], rels: [] });
  const drawn = new DrawnGraph();
  drawn.add({ nodes: [node(1), node(2), node(3)], rels: [rel(10, 1, 2), rel(11, 2, 3)] });
  assert.deepEqual(drawn.keepOnly(original), { nodes: [3], rels: [10, 11] });
  assert.deepEqual([...drawn.nodes.keys()], [1, 2]);
  assert.equal(drawn.rels.size, 0);
});

test('expandNode asks for the neighbours, then connects the new ones to the drawn nodes', async () => {
  const { run, ran } = fakeRun([
    { graph: { nodes: [node(1), node(2), node(3)], rels: [rel(10, 1, 2), rel(11, 1, 3)] }, rows: 2 },
    { graph: { nodes: [], rels: [rel(11, 1, 3), rel(12, 3, 4)] }, rows: 2 },
  ]);
  const found = await expandNode(run, 1, [1, 2, 4]);
  assert.deepEqual(ran, [
    [NEIGHBOURS, { id: 1 }],
    [CONNECTING, { added: [3], drawn: [1, 2, 4, 3] }],
  ]);
  assert.deepEqual(found.nodes.map((n) => n.id), [1, 2, 3]);
  assert.deepEqual(found.rels.map((r) => r.id), [10, 11, 12]);
  assert.equal(found.truncated, false);
});

test('expandNode skips the connecting query when nothing is new, and says when it hit its limit', async () => {
  const { run, ran } = fakeRun([{ graph: { nodes: [node(1), node(2)], rels: [rel(10, 1, 2)] }, rows: EXPAND_LIMIT }]);
  const found = await expandNode(run, 1, [1, 2]);
  assert.equal(ran.length, 1);
  assert.deepEqual(found.rels.map((r) => r.id), [10]);
  assert.equal(found.truncated, true);
});

test('expandNode finds nothing for a node with no relationships, and throws a failed statement', async () => {
  assert.deepEqual(await expandNode(fakeRun([{}]).run, 1, [1]), { nodes: [], rels: [], truncated: false });
  await assert.rejects(expandNode(fakeRun([{ error: 'boom' }]).run, 1, [1]), /boom/);
});
