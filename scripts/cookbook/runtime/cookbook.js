// Cookbook page runtime (ADR-0130).
//
// Pages are complete without this script: code, recorded outputs, and
// links are static HTML. This script adds the site chrome (theme toggle,
// mobile nav) and, on recipe pages, upgrades each <cypher-cell> so it can
// run in the browser:
//
// - The engine is a module Web Worker (worker.js) started on the first Run,
//   never on page load. Nothing runs without a click.
// - All cells on a page share one database. Running a cell first runs the
//   earlier cells that have not run yet, in order, and stops at the first
//   that goes other than its recipe says (lib/cells.mjs).
// - Stop terminates the worker (there is no query timeout); Reset empties
//   the database. Either way every cell goes back to its recorded output.
// - Parameters are sent to the engine as the JSON text the file holds (or
//   the reader typed), never re-serialized, so 1.0 stays a float.
// - Recorded results' envelopes and graphs live in results.json beside the
//   page and are fetched the first time a graph or JSON view needs one.
// - A double-click on a node in a graph view adds its neighbours from the
//   page's database, running the steps it needs first (ADR-0151).

import { failed, surprise } from '../lib/cells.mjs';
import { expandNode } from '../lib/graph-model.mjs';
import { canDraw, lastEnvelope, tableHtml } from '../lib/result-view.mjs';

const ASSETS = new URL('./', import.meta.url);

// ---------------------------------------------------------------- chrome

function setupChrome() {
  const root = document.documentElement;
  document.getElementById('themeToggle')?.addEventListener('click', () => {
    const current = root.getAttribute('data-theme');
    const sysDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    const next = current === 'dark' ? 'light' : current === 'light' ? 'dark' : sysDark ? 'light' : 'dark';
    root.setAttribute('data-theme', next);
    try {
      localStorage.setItem('theme', next);
    } catch {
      /* storage unavailable: the choice lasts for this page only */
    }
  });
  const header = document.getElementById('top');
  const onScroll = () => header?.classList.toggle('scrolled', window.scrollY > 8);
  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();
  const navToggle = document.getElementById('navToggle');
  const nav = document.getElementById('topNav');
  navToggle?.addEventListener('click', () => {
    const open = nav.classList.toggle('open');
    navToggle.setAttribute('aria-expanded', String(open));
  });
  // The contents list is a collapsed disclosure on narrow screens and an
  // open side column on wide ones.
  const toc = document.querySelector('.toc details');
  if (toc) {
    const wide = window.matchMedia('(min-width: 1240px)');
    const sync = () => (toc.open = wide.matches);
    sync();
    wide.addEventListener('change', sync);
  }
  for (const btn of document.querySelectorAll('[data-copy-target]')) {
    btn.addEventListener('click', async () => {
      const target = document.getElementById(btn.dataset.copyTarget);
      try {
        await navigator.clipboard.writeText(target?.value ?? target?.textContent ?? '');
        flash(btn, 'Copied');
      } catch {
        flash(btn, 'Copy failed');
      }
    });
  }
}

function flash(btn, text) {
  const old = btn.dataset.label ?? btn.textContent;
  btn.dataset.label = old;
  btn.textContent = text;
  setTimeout(() => (btn.textContent = old), 1200);
}

// ---------------------------------------------------------------- engine

/** The database is gone: the reader pressed Stop, or the engine crashed. */
class StoppedError extends Error {}
class CrashedError extends StoppedError {}

/**
 * The engine worker. `start()` creates it; nothing else does, so a
 * continuation left over from a stopped run fails instead of quietly
 * starting a new database. The worker ends one way, `discard()`, which sets
 * the status and fires `discarded`: whatever the database held is gone.
 * Statuses: idle | loading | ready | stopped | crashed | failed.
 */
class Engine extends EventTarget {
  worker = null;
  starting = null;
  pending = new Map();
  seq = 0;

  setStatus(status, detail = {}) {
    this.dispatchEvent(new CustomEvent('status', { detail: { status, ...detail } }));
  }

  /** Start the worker unless it runs; resolves once the engine is ready. */
  start() {
    if (this.starting) return this.starting;
    this.worker = new Worker(new URL('worker.js', ASSETS), { type: 'module', name: 'relate' });
    this.worker.onmessage = (e) => {
      const { id, ...rest } = e.data;
      const p = this.pending.get(id);
      if (!p) return;
      this.pending.delete(id);
      if (rest.ok) p.resolve(rest);
      else p.reject(Object.assign(new Error(rest.message), { trap: rest.trap }));
    };
    this.worker.onerror = (e) => {
      e.preventDefault();
      this.discard('failed', new Error(`The engine failed to load: ${e.message || 'network or module error'}`));
    };
    this.setStatus('loading');
    this.starting = this.call({ type: 'init' }).then(
      (r) => this.setStatus('ready', { loadMs: r.loadMs }),
      (err) => {
        if (!(err instanceof StoppedError)) this.discard('failed', err);
        throw err;
      },
    );
    return this.starting;
  }

  /** Send `msg` to the running worker. A trap discards it. */
  async call(msg) {
    if (!this.worker) throw new StoppedError('The engine is not running.');
    const id = ++this.seq;
    try {
      return await new Promise((resolve, reject) => {
        this.pending.set(id, { resolve, reject });
        this.worker.postMessage({ ...msg, id });
      });
    } catch (err) {
      if (!err.trap) throw err;
      // A trapped wasm instance cannot be reused; start over next time.
      const crashed = new CrashedError(`The engine crashed (${err.message}). The database was discarded.`);
      this.discard('crashed', crashed);
      throw crashed;
    }
  }

  /** End the worker: everything in flight fails with `reason`. */
  discard(status, reason) {
    if (!this.worker) return;
    this.worker.terminate();
    this.worker = null;
    this.starting = null;
    for (const p of this.pending.values()) p.reject(reason);
    this.pending.clear();
    this.setStatus(status);
    this.dispatchEvent(new Event('discarded'));
  }

  run(code, params) {
    return this.call({ type: 'run', code, params });
  }

  async reset() {
    if (this.worker) await this.call({ type: 'reset' });
  }

  stop() {
    this.discard('stopped', new StoppedError('Stopped.'));
  }
}

// ---------------------------------------------------------------- session

const MESSAGES = {
  loading: () => 'Loading the engine…',
  ready: ({ loadMs }) => `Engine ready (loaded in ${Math.round(loadMs)} ms). Every step runs in this tab.`,
  failed: () => 'The engine failed to load; run a step to try again. The recorded outputs are still accurate.',
  stopped: () => 'Stopped. The database was discarded; run any step to start again.',
  crashed: () => 'The engine crashed (a bug worth reporting). The database was discarded.',
  emptied: () => 'Database emptied. Run any step to start again.',
};

class Session {
  constructor(bar) {
    this.engine = new Engine();
    this.cells = [];
    this.done = new Set();
    this.busy = false;
    this.run = null; // the current run's AbortController
    this.bar = bar;
    this.engine.addEventListener('status', (e) => this.onStatus(e.detail));
    // The database is gone, so nothing has run in it.
    this.engine.addEventListener('discarded', () => this.forget());
    bar.querySelector('[data-action=run-all]')?.addEventListener('click', () => this.runAll());
    bar.querySelector('[data-action=reset]')?.addEventListener('click', () => this.reset());
    bar.querySelector('[data-action=stop]')?.addEventListener('click', () => this.stop());
    bar.hidden = false;
  }

  register(cell) {
    this.cells.push(cell);
  }

  onStatus(detail) {
    this.bar.dataset.status = detail.status;
    const message = MESSAGES[detail.status];
    if (message) this.bar.querySelector('.engine-text').textContent = message(detail);
  }

  setBusy(busy) {
    this.busy = busy;
    this.bar.querySelector('[data-action=stop]').hidden = !busy;
    for (const b of this.bar.querySelectorAll('[data-action=run-all], [data-action=reset]')) b.disabled = busy;
    for (const c of this.cells) c.setBusy(busy);
  }

  /**
   * Run `task(signal)` while nothing else runs: Run and Reset are disabled
   * and Stop aborts `signal`. Resolves to what `task` returns, or to a
   * problem string when it throws.
   */
  async exclusive(task) {
    this.setBusy(true);
    const run = (this.run = new AbortController());
    try {
      return await task(run.signal);
    } catch (err) {
      if (!(err instanceof StoppedError)) console.error(err);
      return err.message;
    } finally {
      if (this.run === run) {
        this.run = null;
        this.setBusy(false);
      }
    }
  }

  /**
   * Run every not-yet-run cell up to and including `target`. Resolves to
   * null once they all ran as their recipe says, else to why not.
   */
  async bringUpTo(target, signal) {
    const end = this.cells.indexOf(target);
    const todo = this.cells.slice(0, end + 1).filter((c) => !this.done.has(c));
    // Fetch every parameter file the run needs while the engine loads.
    const files = todo.map((c) => c.paramsFile());
    for (const f of files) f?.catch(() => {});
    let current = null;
    try {
      await this.engine.start();
      for (const [i, cell] of todo.entries()) {
        if (signal.aborted) return 'stopped.';
        current = cell;
        const problem = await cell.execute(this.engine, files[i], signal);
        if (signal.aborted) return 'stopped.';
        if (problem === null) {
          this.done.add(cell);
          continue;
        }
        if (cell === target) return 'this step failed.';
        target.showBlocked(cell);
        return `step ${cell.dataset.step} failed.`;
      }
      return null;
    } catch (err) {
      if (err instanceof CrashedError) current?.showError(err.message);
      throw err;
    }
  }

  /** Run every not-yet-run cell before `target`, then `target` itself. */
  runUpTo(target) {
    if (this.busy) return;
    this.done.delete(target);
    return this.exclusive((signal) => this.bringUpTo(target, signal));
  }

  /**
   * Node `id`'s neighbours in the page's database, for the graph view of
   * `cell` drawing the nodes `drawn` (lib/graph-model.mjs): `{graph}`, or
   * `{problem}` when the database cannot get there. The database needs the
   * steps up to `cell`, so those that have not run run first, as a Run
   * would run them, except that `cell` does not run again if it has
   * (ADR-0151).
   */
  async expand(cell, id, drawn) {
    if (this.busy) return { problem: 'wait for the run to finish.' };
    let graph = null;
    const problem = await this.exclusive(async (signal) => {
      const blocked = await this.bringUpTo(cell, signal);
      if (blocked) return blocked;
      graph = await expandNode((code, params) => this.engine.run(code, params), id, drawn);
      return null;
    });
    return problem ? { problem } : { graph };
  }

  runAll() {
    return this.runUpTo(this.cells[this.cells.length - 1]);
  }

  forget() {
    this.done.clear();
    for (const c of this.cells) c.showRecorded();
  }

  async reset() {
    if (this.busy) return;
    this.setBusy(true);
    try {
      await this.engine.reset();
      this.forget();
      this.onStatus({ status: 'emptied' });
    } catch (err) {
      if (!(err instanceof StoppedError)) console.error(err);
    } finally {
      this.setBusy(false);
    }
  }

  stop() {
    this.run?.abort();
    this.run = null;
    this.engine.stop();
    this.setBusy(false);
  }
}

// ---------------------------------------------------------------- cells

const fetched = new Map();

/** `url`'s text, fetched once; a failed fetch is forgotten, so the next call tries again. */
function fetchText(url) {
  if (!fetched.has(url)) {
    const text = fetch(url).then((r) => {
      if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
      return r.text();
    });
    fetched.set(url, text);
    text.catch(() => fetched.delete(url));
  }
  return fetched.get(url);
}

/** The page's recorded `{envelope, graph}`, by cell id (results.json). */
const loadRecorded = () => fetchText(new URL('results.json', document.baseURI).href).then((t) => JSON.parse(t));

// Graph results render when they come near the viewport, not on page load.
const nearViewport = new IntersectionObserver(
  (entries) => {
    for (const e of entries) {
      if (!e.isIntersecting) continue;
      nearViewport.unobserve(e.target);
      e.target.revealGraph();
    }
  },
  { rootMargin: '600px 0px' },
);

/**
 * What a cell's output pane shows: `{kind, text, tableHtml, drawable, data}`,
 * `data` being `{envelope, graph}` for the JSON and graph views. The
 * recording is read from the page, with `data` undefined until a view needs
 * it (then it comes from results.json); a run or an error replaces it.
 */
function liveResult({ envelopes, recording }) {
  return {
    kind: recording.kind,
    text: recording.text,
    tableHtml: recording.table ? tableHtml(recording.table) : '',
    drawable: canDraw(recording.graph),
    data: { envelope: lastEnvelope(envelopes), graph: recording.graph },
  };
}

const errorResult = (message) => ({
  kind: 'table',
  text: `error: ${message}`,
  tableHtml: '',
  drawable: false,
  data: { envelope: null, graph: null },
});

/** The view a result opens in: a drawable gram result's graph, else its text. */
const defaultView = (result) => (result.kind === 'gram' && result.drawable ? 'graph' : 'text');

let session = null;

class CypherCell extends HTMLElement {
  connectedCallback() {
    if (this.upgraded) return;
    this.upgraded = true;
    this.session = session;
    this.session.register(this);
    this.editor = this.querySelector('.cell-editor');
    this.codeView = this.querySelector('.cell-code');
    this.paramsEditor = this.querySelector('.params-editor');
    this.outText = this.querySelector('.out-text');
    this.outTable = this.querySelector('.out-table');
    this.outLabel = this.querySelector('.out-label');
    this.outGraph = this.querySelector('.out-graph');
    this.outJson = this.querySelector('.out-json');
    this.copyBtn = this.querySelector('.out-copy');
    this.tabs = this.querySelector('.out-tabs');
    this.runBtn = this.querySelector('[data-action=run]');
    this.editBtn = this.querySelector('[data-action=edit]');
    this.expectError = this.hasAttribute('data-expect-error');
    // The recording, as the page shows it before anything runs.
    this.recorded = {
      kind: this.dataset.resultKind,
      text: this.outText.textContent,
      tableHtml: this.outTable.innerHTML,
      drawable: this.hasAttribute('data-has-graph'),
    };
    this.result = this.recorded;
    this.view = defaultView(this.result);

    this.runBtn.hidden = false;
    this.editBtn.hidden = false;
    this.runBtn.addEventListener('click', () => this.session.runUpTo(this));
    this.editBtn.addEventListener('click', () => this.toggleEdit());
    this.editor.addEventListener('keydown', (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
        e.preventDefault();
        this.session.runUpTo(this);
      }
    });
    this.editor.addEventListener('input', () => this.fitEditor());
    for (const tab of this.tabs.querySelectorAll('[data-view]')) {
      tab.addEventListener('click', () => this.showView(tab.dataset.view));
    }
    this.renderTabs();
    if (this.view === 'graph') {
      // Keep the recorded text on screen until the graph is drawn.
      this.selectTab('graph');
      nearViewport.observe(this);
    } else {
      this.showView(this.view);
    }
  }

  revealGraph() {
    if (this.view === 'graph' && this.result === this.recorded) this.showView('graph');
  }

  /** Show `result` in `view`: its text or table, its tabs, and that view. */
  paint(result, view) {
    if (result !== this.result) this.dropGraph();
    this.result = result;
    this.outText.textContent = result.text;
    this.outTable.innerHTML = result.tableHtml;
    this.renderTabs();
    this.showView(view);
  }

  get editing() {
    return !this.editor.hidden;
  }

  toggleEdit() {
    if (this.editing) {
      // Leaving edit mode restores the recipe's query.
      this.editor.value = this.editor.defaultValue;
      this.editor.hidden = true;
      this.codeView.hidden = false;
      this.editBtn.textContent = 'Edit';
    } else {
      this.editor.hidden = false;
      this.codeView.hidden = true;
      this.editBtn.textContent = 'Revert';
      this.fitEditor();
      this.editor.focus();
    }
  }

  fitEditor() {
    this.editor.style.height = 'auto';
    this.editor.style.height = `${this.editor.scrollHeight + 2}px`;
  }

  setBusy(busy) {
    this.runBtn.disabled = busy;
  }

  /** The fetch of this cell's parameter file, if it loads one. */
  paramsFile() {
    return this.dataset.params ? fetchText(new URL(this.dataset.params, document.baseURI).href) : null;
  }

  /**
   * Run this cell; `file` is its parameter file's fetch. Returns how the run
   * went other than the recipe says (lib/cells.mjs), or null.
   */
  async execute(engine, file, signal) {
    this.classList.add('is-running');
    this.classList.remove('is-error', 'is-blocked');
    this.outLabel.textContent = 'Running…';
    try {
      const params = this.paramsEditor ? this.paramsEditor.value : await file;
      if (signal.aborted) throw new StoppedError('Stopped.');
      const code = this.editing ? this.editor.value : this.editor.defaultValue;
      const res = await engine.run(code, params ?? null);
      const result = liveResult(res);
      const { envelope } = result.data;
      const problem = surprise({ id: this.dataset.cell, expectError: this.expectError }, envelope);
      const n = res.envelopes.length;
      const parts = [`Live · ${res.ms < 1 ? '<1' : Math.round(res.ms)} ms`];
      if (this.expectError) parts.unshift(failed(envelope) ? 'Expected error' : 'Expected an error, got none');
      if (n > 1) parts.push(`${n} statements`);
      const unchanged = !this.editing && !this.paramsEdited();
      if (unchanged && result.kind === this.recorded.kind && result.text === this.recorded.text) {
        parts.push('matches the recording');
      }
      this.outLabel.textContent = parts.join(' · ');
      this.classList.toggle('is-error', problem !== null);
      this.classList.add('is-live');
      // A reader looking at the JSON goes on looking at it.
      this.paint(result, this.view === 'json' ? 'json' : defaultView(result));
      return problem;
    } catch (err) {
      if (err instanceof StoppedError) {
        this.showRecorded();
        throw err;
      }
      this.showError(err.message);
      return err.message;
    } finally {
      this.classList.remove('is-running');
    }
  }

  paramsEdited() {
    return this.paramsEditor ? this.paramsEditor.value !== this.paramsEditor.defaultValue : false;
  }

  showError(message) {
    this.outLabel.textContent = 'Could not run';
    this.classList.add('is-error');
    this.paint(errorResult(message), 'text');
  }

  showBlocked(culprit) {
    this.classList.add('is-blocked');
    this.outLabel.textContent = `Not run: step ${culprit.dataset.step} failed`;
  }

  showRecorded() {
    this.classList.remove('is-live', 'is-error', 'is-blocked', 'is-running');
    this.outLabel.textContent = this.expectError ? 'Expected error' : 'Recorded result';
    this.paint(this.recorded, defaultView(this.recorded));
  }

  /** `{envelope, graph}` of the result on screen; the recording's come from results.json. */
  async data() {
    if (this.result !== this.recorded) return this.result.data;
    return (await loadRecorded())[this.dataset.cell] ?? { envelope: null, graph: null };
  }

  renderTabs() {
    this.tabs.querySelector('[data-view=graph]').hidden = !this.result.drawable;
    this.tabs.hidden = false;
    this.copyBtn.hidden = this.result.tableHtml === '';
  }

  selectTab(view) {
    for (const tab of this.tabs.querySelectorAll('[data-view]')) {
      tab.setAttribute('aria-selected', String(tab.dataset.view === view));
    }
  }

  async showView(view) {
    this.view = view;
    const result = this.result;
    this.selectTab(view);
    const table = view === 'text' && this.result.tableHtml !== '';
    this.outTable.hidden = !table;
    this.outText.hidden = view !== 'text' || table;
    this.outJson.hidden = view !== 'json';
    this.outGraph.hidden = view !== 'graph';
    if (view === 'graph') {
      try {
        await this.graphView();
      } catch (err) {
        if (this.view === view && this.result === result) {
          const note = Object.assign(document.createElement('p'), { className: 'out-graph-error' });
          note.textContent = `Could not load the result: ${err.message}`;
          this.outGraph.replaceChildren(note);
        }
      }
      return;
    }
    if (view !== 'json') return;
    let data;
    try {
      this.outJson.textContent = 'Loading…';
      data = await this.data();
    } catch (err) {
      if (this.view === view) this.outJson.textContent = `Could not load the result: ${err.message}`;
      return;
    }
    // The reader moved on, or a run replaced the result, while it loaded.
    if (this.view === view && this.result === result) {
      this.outJson.textContent = JSON.stringify(data.envelope, null, 2);
    }
  }

  /**
   * The graph view of the result on screen (runtime/graph.js), drawn once
   * per result so expansions last while the reader switches tabs; null when
   * a run replaced the result while it loaded.
   */
  graphView() {
    const result = this.result;
    if (this.graph?.result !== result) {
      this.dropGraph();
      const view = Promise.all([this.data(), import('./graph.js')]).then(([data, { renderGraph }]) =>
        this.result === result
          ? renderGraph(this.outGraph, data.graph, { onExpand: (id, drawn) => this.expand(id, drawn) })
          : null,
      );
      // A failed load is tried again the next time the graph shows.
      view.catch(() => this.graph?.view === view && (this.graph = null));
      this.graph = { result, view };
    }
    return this.graph.view;
  }

  /** Destroy the graph view, and empty its pane. */
  dropGraph() {
    this.graph?.view.then((v) => v?.destroy(), () => {});
    this.graph = null;
    this.outGraph.replaceChildren();
  }

  /**
   * Expand node `id` of the graph view (`drawn`: the nodes it draws). The
   * session may run this very step first, which gives it a new result and
   * a new view; the neighbours go to the result's view when they arrive.
   */
  async expand(id, drawn) {
    const outcome = await this.session.expand(this, id, drawn);
    if (!this.result.drawable) return; // the step failed; its output says why
    (await this.graphView().catch(() => null))?.grow(id, outcome);
  }
}

// ---------------------------------------------------------------- boot

setupChrome();
const bar = document.getElementById('engineBar');
if (bar && typeof Worker !== 'undefined') {
  session = new Session(bar);
  customElements.define('cypher-cell', CypherCell);
}

