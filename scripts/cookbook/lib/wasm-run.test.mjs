import assert from 'node:assert/strict';
import { test } from 'node:test';

import { fakeRun, node, rel } from './graph-fixtures.mjs';
import { expandProblem, parityProblems, runRecipe } from './wasm-run.mjs';

// A stand-in for the bundle: each script is one statement whose envelope is
// an error when the script says `FAIL`, and renders as its own text.
const fakeEngine = () => {
  const ran = [];
  class RelateDb {
    executeScript(code, params) {
      ran.push([code, params]);
      return [JSON.stringify({ columns: [], rows: [], stats: {}, error: code === 'FAIL' ? 'boom' : null })];
    }
    free() {}
  }
  const renderResult = (envelopes) => {
    const last = JSON.parse(envelopes.at(-1));
    return JSON.stringify({ kind: 'table', text: last.error ? `error: ${last.error}` : '(no rows)' });
  };
  return { mod: { RelateDb, renderResult }, ran };
};

const cell = (id, code, extra = {}) => ({ id, code, params: null, expectError: false, ...extra });

test('runRecipe runs cells in order with their data blocks and stops at a surprise', async () => {
  const { mod, ran } = fakeEngine();
  const recipe = {
    data: new Map([['k', '{"k": 1.0}']]),
    cells: [cell('a', 'A', { params: 'k' }), cell('b', 'FAIL'), cell('c', 'C')],
  };
  const run = await runRecipe(mod, recipe);
  assert.deepEqual(ran, [
    ['A', '{"k": 1.0}'],
    ['FAIL', null],
  ]);
  assert.deepEqual(
    run.cells.map((c) => [c.id, c.recording.text]),
    [
      ['a', '(no rows)'],
      ['b', 'error: boom'],
    ],
  );
  assert.equal(run.stopped, 'cell `b` failed: boom');
});

test('runRecipe runs past an expected error', async () => {
  const { mod } = fakeEngine();
  const run = await runRecipe(mod, { data: new Map(), cells: [cell('a', 'FAIL', { expectError: true }), cell('b', 'B')] });
  assert.equal(run.stopped, null);
  assert.equal(run.cells.length, 2);
});

test('runRecipe lets afterCell query the database after each cell that went as its recipe says', async () => {
  const { mod, ran } = fakeEngine();
  const seen = [];
  const afterCell = async (run, cell, recording) => {
    seen.push([cell.id, recording.text]);
    assert.deepEqual(run(`AFTER ${cell.id}`, '{}').recording, { kind: 'table', text: '(no rows)' });
  };
  await runRecipe(mod, { data: new Map(), cells: [cell('a', 'A'), cell('b', 'FAIL'), cell('c', 'C')] }, { afterCell });
  assert.deepEqual(seen, [['a', '(no rows)']]);
  assert.deepEqual(
    ran.map(([code]) => code),
    ['A', 'AFTER a', 'FAIL'],
  );
});

test('expandProblem expects the expansion to find every relationship the step recorded for the node', async () => {
  const recipe = { slug: 's' };
  const step = { id: 'c' };
  const nodes = [node(1), node(2), node(3)];
  const recording = { graph: { nodes, rels: [rel(10, 1, 2), rel(11, 3, 1), rel(12, 2, 3)] } };
  const all = { nodes, rels: [rel(10, 1, 2), rel(11, 3, 1)] };
  const answering = (...answers) => fakeRun(answers).run;
  assert.equal(await expandProblem(recipe, step, answering({ graph: all, rows: 2 }), recording), null);
  const some = { nodes: [node(1), node(2)], rels: [rel(10, 1, 2)] };
  assert.equal(
    await expandProblem(recipe, step, answering({ graph: some }), recording),
    '[s] cell `c`: expanding node 1 missed its relationship(s) 11',
  );
  assert.equal(await expandProblem(recipe, step, answering({ graph: some, rows: 100 }), recording), null);
  assert.equal(
    await expandProblem(recipe, step, answering({ graph: null, error: 'boom' }), recording),
    '[s] cell `c`: expanding node 1 failed: boom',
  );
  assert.equal(await expandProblem(recipe, step, answering(), { graph: null }), null);
});

test('parityProblems names every difference, and the cells a stop skipped', () => {
  const recipe = {
    slug: 's',
    cells: [
      { id: 'a', result: { kind: 'table', text: '(no rows)' } },
      { id: 'b', result: { kind: 'table', text: '1 row' } },
      { id: 'c', result: null },
      { id: 'd', result: { kind: 'table', text: '(no rows)' } },
    ],
  };
  const recording = (text) => ({ kind: 'table', text });
  const run = {
    cells: [
      { id: 'a', recording: recording('(no rows)') },
      { id: 'b', recording: recording('2 rows') },
      { id: 'c', recording: recording('(no rows)') },
    ],
    stopped: 'cell `c` failed: boom',
  };
  assert.deepEqual(parityProblems(recipe, run), [
    '[s] cell `c` failed: boom; the 1 cell(s) after it did not run',
    "[s] cell `b`: the browser build's result differs from the recording\n--- recorded (table)\n1 row\n--- wasm (table)\n2 rows",
    '[s] cell `c` has no result block',
  ]);
  const matching = recipe.cells.slice(0, 1);
  assert.deepEqual(parityProblems({ slug: 's', cells: matching }, { cells: run.cells.slice(0, 1), stopped: null }), []);
});
