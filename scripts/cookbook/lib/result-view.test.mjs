import assert from 'node:assert/strict';
import { test } from 'node:test';

import { canDraw, lastEnvelope, tableHtml } from './result-view.mjs';

test('lastEnvelope parses the last statement\'s envelope', () => {
  assert.deepEqual(lastEnvelope(['{"n": 1}', '{"n": 2}']), { n: 2 });
  assert.equal(lastEnvelope([]), null);
});

test('canDraw needs a node: a table has no graph, and lone relationships do not draw', () => {
  assert.equal(canDraw(null), false);
  assert.equal(canDraw({ nodes: [], rels: [{ id: 1 }] }), false);
  assert.equal(canDraw({ nodes: [{ id: 0 }], rels: [] }), true);
});

test('tableHtml escapes cells and right-aligns numeric columns', () => {
  const html = tableHtml({
    columns: ['name', 'n'],
    rows: [['"<b>&amp"', '446'], ['null', '1.0']],
    numeric: [false, true],
    summary: '2 rows · nodes created: 1',
  });
  assert.equal(
    html,
    '<div class="table-scroll" tabindex="0"><table><thead><tr><th scope="col">name</th><th scope="col" class="num">n</th></tr></thead>' +
      '<tbody><tr><td>&quot;&lt;b&gt;&amp;amp&quot;</td><td class="num">446</td></tr><tr><td>null</td><td class="num">1.0</td></tr></tbody></table></div>' +
      '<p class="out-summary">2 rows · nodes created: 1</p>',
  );
});
