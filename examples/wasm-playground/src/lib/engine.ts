import type { Ok, OkFor, Request, Response, ScriptRun } from './protocol';
import type { WireResult } from './wire';

export type { ScriptRun };

export class EnginePoisonedError extends Error {
  constructor(public readonly cause: Error) {
    super(
      `Relate wasm module crashed and cannot be reused: ${cause.message}. ` +
        `Reload the page to continue. The failing query is a bug — please report it.`,
    );
    this.name = 'EnginePoisonedError';
  }
}

// The database lives in a dedicated worker. This class is the page-side
// queue: one request at a time, matching the single-threaded wasm module.
export class Engine {
  private worker: Worker;
  private nextId = 1;
  private pending = new Map<number, { resolve: (value: Ok) => void; reject: (err: Error) => void }>();
  private tail: Promise<unknown> = Promise.resolve();
  private poisoned: EnginePoisonedError | null = null;
  private memBytes = 0;

  constructor() {
    this.worker = new Worker(new URL('./engine.worker.ts', import.meta.url), { type: 'module' });
    this.worker.onmessage = (event: MessageEvent<Response>) => {
      const data = event.data;
      const waiter = this.pending.get(data.id);
      if (!waiter) return;
      this.pending.delete(data.id);
      if (!data.ok) {
        if (data.trap) {
          this.poisoned ??= new EnginePoisonedError(new Error(data.message));
          waiter.reject(this.poisoned);
        } else {
          waiter.reject(new Error(data.message));
        }
        return;
      }
      waiter.resolve(data);
    };
    this.worker.onerror = (event) => {
      const error = new Error(event.message || 'engine worker failed');
      this.poisoned ??= new EnginePoisonedError(error);
      for (const waiter of this.pending.values()) waiter.reject(this.poisoned);
      this.pending.clear();
    };
    if (import.meta.hot) {
      const worker = this.worker;
      import.meta.hot.dispose(() => worker.terminate());
    }
  }

  /** Load the bundle and open the default in-memory database (once). */
  ready(): Promise<void> {
    return this.enqueue({ type: 'init' }).then(() => undefined);
  }

  /** Bytes of wasm memory in use; 0 before the engine loads. */
  memoryBytes(): number {
    void this.enqueue({ type: 'memory' })
      .then((res) => {
        this.memBytes = res.bytes;
      })
      .catch(() => undefined);
    return this.memBytes;
  }

  /** Run one statement; `paramsJson` is a JSON object as text, or null. */
  execute(query: string, paramsJson: string | null): Promise<WireResult> {
    return this.enqueue({ type: 'execute', query, params: paramsJson }).then((res) => {
      return JSON.parse(res.raw) as WireResult;
    });
  }

  /**
   * Run a script of one or more statements with the engine's own runner
   * (`executeScript`, ADR-0131): statements in order, each with `paramsJson`
   * (a JSON object as text, passed verbatim, or null), stopping at the first
   * error; the statements before it stay applied. Null when the script holds
   * no statement (only comments or whitespace).
   */
  runScript(script: string, paramsJson: string | null): Promise<ScriptRun | null> {
    return this.enqueue({ type: 'run', code: script, params: paramsJson }).then((res) => res.result);
  }

  reset(): Promise<void> {
    return this.enqueue({ type: 'reset' }).then(() => undefined);
  }

  isPoisoned(): EnginePoisonedError | null {
    return this.poisoned;
  }

  private enqueue<T extends Request>(request: T): Promise<OkFor<T['type']>> {
    const run = () => {
      if (this.poisoned) return Promise.reject(this.poisoned);
      const id = this.nextId++;
      return new Promise<OkFor<T['type']>>((resolve, reject) => {
        this.pending.set(id, { resolve: resolve as (value: Ok) => void, reject });
        this.worker.postMessage({ ...request, id });
      });
    };
    const next = this.tail.then(run, run);
    this.tail = next.catch(() => undefined);
    return next;
  }
}

export const engine = new Engine();
