// Run recipes through the browser build of the engine in Node, as the page
// does (ADR-0130): one in-memory database per recipe, cells in document
// order through `executeScript` (the engine's own script runner, ADR-0131)
// with their data blocks as parameters, stopping at the first cell that goes
// other than its recipe says (lib/cells.mjs). Results are rendered with the
// bundle's `renderResult`, the renderer the native harness records with. The
// site build uses this to check every recording against the browser build
// (the WASM parity check), to check that the graph view can expand the
// nodes it draws (ADR-0151), and to publish what the graph and JSON views
// need.

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

import { surprise } from './cells.mjs';
import { expandNode } from './graph-model.mjs';
import { lastEnvelope } from './result-view.mjs';

/** Load and instantiate the bundle in `wasmDir`. */
export async function loadEngine(wasmDir) {
  const mod = await import(pathToFileURL(join(wasmDir, 'relate.js')).href);
  mod.initSync({ module: readFileSync(join(wasmDir, 'relate_bg.wasm')) });
  if (typeof mod.renderResult !== 'function' || typeof mod.RelateDb.prototype.executeScript !== 'function') {
    throw new Error(`${wasmDir}: the bundle has no executeScript / renderResult (ADR-0131); rebuild it`);
  }
  return mod;
}

/**
 * Run `recipe`'s cells: `{cells: [{id, envelopes, recording}], stopped}`,
 * `envelopes` being the engine's wire JSON text per statement and `stopped`
 * why the run ended early, or null. After each cell that goes as its recipe
 * says, `afterCell(run, cell, recording)` may query the database as it
 * stands then: `run(code, params)` runs a script, `params` being JSON text,
 * and returns `{envelopes, recording}` as the page's engine worker does.
 */
export async function runRecipe(mod, recipe, { afterCell } = {}) {
  const db = new mod.RelateDb();
  const run = (code, params) => {
    const envelopes = db.executeScript(code, params);
    return { envelopes, recording: JSON.parse(mod.renderResult(envelopes)) };
  };
  try {
    const cells = [];
    for (const cell of recipe.cells) {
      const { envelopes, recording } = run(cell.code, cell.params ? recipe.data.get(cell.params) : null);
      cells.push({ id: cell.id, envelopes, recording });
      const stopped = surprise(cell, lastEnvelope(envelopes));
      if (stopped) return { cells, stopped };
      await afterCell?.(run, cell, recording);
    }
    return { cells, stopped: null };
  } finally {
    db.free();
  }
}

/**
 * How expanding a node of `recording`'s graph (ADR-0151) goes other than it
 * should, run with `run` right after cell `cell` of `recipe`, or null. It
 * expands the graph's first node and expects every relationship of that
 * node the step recorded, unless the expansion hit its limit.
 */
export async function expandProblem(recipe, cell, run, recording) {
  const node = recording.graph?.nodes[0];
  if (!node) return null;
  let found;
  try {
    found = await expandNode(run, node.id, recording.graph.nodes.map((n) => n.id));
  } catch (err) {
    return `[${recipe.slug}] cell \`${cell.id}\`: expanding node ${node.id} failed: ${err.message}`;
  }
  const got = new Set(found.rels.map((r) => r.id));
  const missing = recording.graph.rels
    .filter((r) => (r.start_node === node.id || r.end_node === node.id) && !got.has(r.id))
    .map((r) => r.id);
  if (missing.length === 0 || found.truncated) return null;
  const where = `[${recipe.slug}] cell \`${cell.id}\``;
  return `${where}: expanding node ${node.id} missed its relationship(s) ${missing.join(', ')}`;
}

/** Every way `run` differs from `recipe`'s recordings, each with both texts. */
export function parityProblems(recipe, run) {
  const problems = [];
  if (run.stopped) {
    const skipped = recipe.cells.length - run.cells.length;
    problems.push(`[${recipe.slug}] ${run.stopped}${skipped ? `; the ${skipped} cell(s) after it did not run` : ''}`);
  }
  for (const { id, recording } of run.cells) {
    const cell = recipe.cells.find((c) => c.id === id);
    if (!cell.result) {
      problems.push(`[${recipe.slug}] cell \`${id}\` has no result block`);
    } else if (cell.result.kind !== recording.kind || cell.result.text !== recording.text) {
      problems.push(
        `[${recipe.slug}] cell \`${id}\`: the browser build's result differs from the recording\n` +
          `--- recorded (${cell.result.kind})\n${cell.result.text}\n--- wasm (${recording.kind})\n${recording.text}`,
      );
    }
  }
  return problems;
}
