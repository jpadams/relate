// Test fixtures for the graph view's expansion (ADR-0151), shared by
// graph-model.test.mjs and wasm-run.test.mjs: wire elements, and a stand-in
// for the engine that answers each statement in turn.

/** A wire node. */
export const node = (id, labels = ['A'], properties = {}) => ({ id, labels, properties });

/** A wire relationship. */
export const rel = (id, start_node, end_node, type = 'R') => ({ id, type, start_node, end_node, properties: {} });

/**
 * A `run(code, params)` whose statements answer with `answers` in turn, each
 * `{graph, rows, error}` (all optional), as `{envelopes, recording}` the way
 * the engine worker does; `ran` records each statement's code and parsed
 * parameters.
 */
export function fakeRun(answers) {
  const ran = [];
  const run = (code, params) => {
    ran.push([code, JSON.parse(params)]);
    const { graph = null, rows = 0, error = null } = answers[ran.length - 1];
    return { envelopes: [JSON.stringify({ rows: Array(rows).fill({}), error })], recording: { graph } };
  };
  return { run, ran };
}
