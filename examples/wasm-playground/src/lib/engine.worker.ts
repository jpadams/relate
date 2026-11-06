// Playground engine. Runs in a dedicated worker. The page talks to it over
// postMessage. Each load opens the default in-memory database, so a reload
// starts empty.

import init, { RelateDb, renderResult } from '../wasm/relate';
import { isWasmTrap } from '../wasm/trap';
import type { Inbound, ScriptRun } from './protocol';
import type { QueryStats, WireResult } from './wire';

let db: InstanceType<typeof RelateDb> | null = null;
let memory: WebAssembly.Memory | null = null;

const ready = (async () => {
  const out = await init();
  memory = out.memory;
  db = new RelateDb();
})();

function fail(id: number, err: unknown) {
  const error = err instanceof Error ? err : new Error(String(err));
  self.postMessage({
    id,
    ok: false,
    trap: isWasmTrap(err),
    message: error.message,
  });
}

self.onmessage = async (event: MessageEvent<Inbound>) => {
  const msg = event.data;
  try {
    await ready;
    if (!db || !memory) throw new Error('engine did not open');
    switch (msg.type) {
      case 'init':
        self.postMessage({ id: msg.id, ok: true, type: 'init' });
        break;
      case 'memory':
        self.postMessage({ id: msg.id, ok: true, type: 'memory', bytes: memory.buffer.byteLength });
        break;
      case 'reset':
        try {
          db.reset();
        } catch (err) {
          // A failed wipe must not keep serving the parked in-memory database.
          db = null;
          throw err;
        }
        self.postMessage({ id: msg.id, ok: true, type: 'reset' });
        break;
      case 'execute':
        self.postMessage({
          id: msg.id,
          ok: true,
          type: 'execute',
          raw: db.execute(msg.query, msg.params),
        });
        break;
      case 'run': {
        const envelopes = db.executeScript(msg.code, msg.params);
        const result: ScriptRun | null =
          envelopes.length === 0
            ? null
            : {
                last: JSON.parse(envelopes[envelopes.length - 1]) as WireResult,
                stats: (JSON.parse(renderResult(envelopes)) as { stats: QueryStats }).stats,
                statements: envelopes.length,
              };
        self.postMessage({ id: msg.id, ok: true, type: 'run', result });
        break;
      }
      default: {
        const unexpected: never = msg;
        throw new Error(`unknown engine request: ${JSON.stringify(unexpected)}`);
      }
    }
  } catch (err) {
    fail(msg.id, err);
  }
};
