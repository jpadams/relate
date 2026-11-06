// Engine worker for cookbook pages (ADR-0130).
//
// Owns the page's one in-memory database. Running the engine here, not on
// the page's thread, is what makes Stop possible: there is no query
// timeout, so the page terminates this worker to end a runaway query.
//
// Messages in:  {id, type: 'init'}
//               {id, type: 'run', code, params}   params: JSON text or null
//               {id, type: 'reset'}
// Messages out: {id, ok: true, loadMs}                              (init)
//               {id, ok: true, envelopes, recording, ms}            (run)
//               {id, ok: true}                                      (reset)
//               {id, ok: false, trap, message}                      (any failure)
//
// A cell runs through the engine's own script runner (`executeScript`,
// ADR-0131) and is rendered by `renderResult`, the code the native harness
// records with; `envelopes` are the engine's wire JSON text per statement.

import init, { RelateDb, renderResult } from '../wasm/relate.js';
import { isWasmTrap } from '../wasm/trap.js';

let db = null;
const ready = (async () => {
  const t0 = performance.now();
  await init();
  db = new RelateDb();
  return performance.now() - t0;
})();

self.onmessage = async (event) => {
  const msg = event.data;
  try {
    const loadMs = await ready;
    if (msg.type === 'init') {
      self.postMessage({ id: msg.id, ok: true, loadMs });
    } else if (msg.type === 'reset') {
      db.reset();
      self.postMessage({ id: msg.id, ok: true });
    } else if (msg.type === 'run') {
      const t0 = performance.now();
      const envelopes = db.executeScript(msg.code, msg.params);
      const recording = JSON.parse(renderResult(envelopes));
      self.postMessage({ id: msg.id, ok: true, envelopes, recording, ms: performance.now() - t0 });
    } else {
      throw new Error(`unknown message type ${msg.type}`);
    }
  } catch (err) {
    self.postMessage({ id: msg.id, ok: false, trap: isWasmTrap(err), message: String(err?.message ?? err) });
  }
};
