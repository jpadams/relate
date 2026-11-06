// Graph view for cookbook cells (ADR-0151): a step's graph result drawn by
// NVL, Neo4j's visualization library. The reader zooms (pinch, the wheel
// once they have clicked into the view or with Ctrl/⌘ held, or the
// buttons), pans, drags nodes, hovers or clicks an element to inspect it,
// and double-clicks a node to add its neighbours from the page's database.
// Loaded only when a graph view is about to show.

import { NVL } from '@neo4j-nvl/base';
import {
  ClickInteraction,
  DragNodeInteraction,
  HoverInteraction,
  PanInteraction,
  ZoomInteraction,
} from '@neo4j-nvl/interaction-handlers';

import {
  DrawnGraph,
  EXPAND_LIMIT,
  MAX_DRAWN,
  MAX_NODES,
  formatValue,
  idOf,
  kindOf,
  labelColor,
  labelColors,
  labelKey,
  nodeKey,
  nvlNode,
  nvlRel,
  relKey,
} from '../lib/graph-model.mjs';
import { TOKENS } from '../lib/tokens.mjs';

// Labels are coloured as the recipe's data-model diagram colours them.
const COLORS = labelColors(document.getElementById('label-colors')?.textContent);

const ICONS = {
  in: '<path d="M8 3v10M3 8h10"/>',
  out: '<path d="M3 8h10"/>',
  fit: '<path d="M2 6V2h4M10 2h4v4M14 10v4h-4M6 14H2v-4"/>',
  reset: '<path d="M2.5 8a5.5 5.5 0 1 0 1.6-3.9"/><path d="M2.5 2v3h3"/>',
};

function el(tag, attrs = {}, parent) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === 'text') node.textContent = v;
    else node.setAttribute(k, v);
  }
  parent?.append(node);
  return node;
}

function button(parent, name, label) {
  const b = el('button', { type: 'button', class: 'graph-tool', title: label, 'aria-label': label }, parent);
  b.innerHTML =
    `<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.6" ` +
    `stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`;
  return b;
}

/** The page's theme: the reader's choice, else the system's. */
function currentTheme() {
  const chosen = document.documentElement.getAttribute('data-theme');
  if (chosen === 'light' || chosen === 'dark') return chosen;
  return matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

/** The colours NVL draws with in the page's theme; its canvas is out of the CSS's reach. */
function canvasTheme() {
  const t = TOKENS[currentTheme()];
  return { rel: t.fgFaint, selected: t.accent, shadow: t.accent };
}

const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;
const labelText = (labels) => labels.map((l) => `:${l}`).join('') || '(no label)';

// ---------------------------------------------------------------- parts

/** An NVL instance in `host` drawing `first` (`{nodes, rels}`, wire JSON). */
function createNvl(host, first, theme, callbacks) {
  return new NVL(
    host,
    first.nodes.map((n) => nvlNode(n, COLORS)),
    first.rels.map((r) => nvlRel(r, theme.rel)),
    {
      renderer: 'canvas',
      layout: 'd3Force',
      // NVL lays out on the page's thread: its workers need bundler support
      // the static pages do not have, and cookbook results are small.
      disableWebWorkers: true,
      disableTelemetry: true,
      minZoom: 0.05,
      maxZoom: 4,
      styling: { selectedBorderColor: theme.selected, dropShadowColor: theme.shadow },
    },
    { ...callbacks, onError: (err) => console.error('graph view:', err) },
  );
}

/**
 * NVL's interaction handlers on `nvl`, reporting to the view: `onMove()`
 * when the reader zooms or pans, `onPoint(key)` with the NVL id under the
 * pointer (or null), `onPin(key)` on a click (null on the canvas), and
 * `onExpand(id)` with the engine id of a node double-clicked. Returns the
 * handlers, to destroy.
 */
function interactions(nvl, { onMove, onPoint, onPin, onExpand }) {
  const zoom = new ZoomInteraction(nvl);
  const pan = new PanInteraction(nvl, { excludeNodeMargin: true });
  const drag = new DragNodeInteraction(nvl);
  const click = new ClickInteraction(nvl, { selectOnClick: true });
  const hover = new HoverInteraction(nvl, { drawShadowOnHover: true });
  zoom.updateCallback('onZoom', onMove);
  pan.updateCallback('onPan', onMove);
  hover.updateCallback('onHover', (element) => onPoint(element?.id ?? null));
  click.updateCallback('onNodeClick', (node) => onPin(node.id));
  click.updateCallback('onRelationshipClick', (rel) => onPin(rel.id));
  click.updateCallback('onCanvasClick', () => onPin(null));
  click.updateCallback('onNodeDoubleClick', (node) => onExpand(idOf(node.id)));
  return [zoom, pan, drag, click, hover];
}

/**
 * Keeping the graph fitted to the pane (`fitAll(animated)` fits it) until
 * the reader zooms or pans it themselves.
 */
function createCamera(stage, fitAll) {
  let fitted = false; // fitted to the graph since it was drawn
  let moved = false; // the reader has zoomed or panned since
  const fit = (animated) => {
    if (stage.clientWidth === 0) return;
    fitAll(animated);
    fitted = true;
  };
  return {
    /** The reader moved the view: leave it where they put it. */
    moved: () => (moved = true),
    /** Fit the graph to the pane, and keep it fitted again. */
    fit: () => {
      moved = false;
      fit(true);
    },
    /** The layout settled: fit, unless the reader has moved the view. */
    settled: () => {
      if (!moved) fit(fitted);
    },
    /** The pane resized: fit a graph drawn while its pane was hidden. */
    resized: () => {
      if (!fitted && !moved) fit(false);
    },
  };
}

/**
 * The toolbar: zoom in and out (`zoomBy(factor)`), `fit()`, and `reset()`.
 * Returns the reset button, shown only once the view has grown.
 */
function toolbar(parent, { zoomBy, fit, reset }) {
  const tools = el('div', { class: 'graph-tools' }, parent);
  button(tools, 'in', 'Zoom in').addEventListener('click', () => zoomBy(1.4));
  button(tools, 'out', 'Zoom out').addEventListener('click', () => zoomBy(1 / 1.4));
  button(tools, 'fit', 'Fit the graph to the view').addEventListener('click', fit);
  const resetBtn = button(tools, 'reset', 'Show the step’s result again');
  resetBtn.hidden = true;
  resetBtn.addEventListener('click', reset);
  return resetBtn;
}

/**
 * The inspector: the labels or type and properties of the element clicked
 * (pinned until it is closed or the canvas is clicked), else of the one
 * under the pointer. `itemOf(key)` is the wire element behind an NVL id;
 * `onUnpin()` runs when a pinned element is let go.
 */
function createInspector(parent, itemOf, onUnpin) {
  const panel = el('div', { class: 'graph-inspector', hidden: '' }, parent);
  let pinned = null;

  // Unpinned, the panel lets the pointer through to the graph beneath.
  function show(key) {
    const item = key && itemOf(key);
    panel.hidden = !item;
    if (!item) return;
    const kind = kindOf(key);
    panel.replaceChildren();
    panel.classList.toggle('is-pinned', key === pinned);
    const head = el('div', { class: 'graph-inspector-head' }, panel);
    if (kind === 'node') {
      const chip = el('span', { class: 'graph-chip', text: labelText(item.labels) }, head);
      chip.style.background = labelColor(item.labels, COLORS);
    } else {
      el('span', { class: 'graph-chip rel', text: `:${item.type}` }, head);
    }
    el('span', { class: 'graph-inspector-id', text: `${kind} ${item.id}` }, head);
    if (key === pinned) {
      const close = el('button', { type: 'button', class: 'graph-inspector-close', 'aria-label': 'Close' }, head);
      close.textContent = '×';
      close.addEventListener('click', () => pin(null));
    }
    const props = Object.entries(item.properties ?? {});
    if (props.length) {
      const dl = el('dl', {}, panel);
      for (const [k, v] of props) {
        el('dt', { text: k }, dl);
        el('dd', { text: formatValue(v) }, dl);
      }
    } else {
      el('p', { class: 'graph-inspector-none', text: 'No properties' }, panel);
    }
    if (kind === 'node') el('p', { class: 'graph-inspector-hint', text: 'Double-click to expand' }, panel);
  }

  function pin(key) {
    if (!key && pinned) onUnpin();
    pinned = key;
    show(key);
  }

  return {
    pin,
    hover: (key) => {
      if (!pinned) show(key);
    },
    /** Let go of a pinned element the view no longer draws. */
    prune: () => {
      if (pinned && !itemOf(pinned)) pin(null);
    },
  };
}

/** The status line: `say(text, ttl)` shows `text` for `ttl` ms, or until the next message when `ttl` is null. */
function statusLine(parent) {
  const status = el('p', { class: 'graph-status', role: 'status', hidden: '' }, parent);
  let quiet = null;
  return {
    say(text, ttl = 6000) {
      clearTimeout(quiet);
      status.textContent = text;
      status.hidden = !text;
      if (text && ttl !== null) quiet = setTimeout(() => (status.hidden = true), ttl);
    },
    stop: () => clearTimeout(quiet),
  };
}

/** The legend: a chip per label set `drawn` draws, then `note` if any, then the hint. */
function drawLegend(legend, drawn, note) {
  legend.replaceChildren();
  const sets = new Map([...drawn.nodes.values()].map((n) => [labelKey(n.labels), n.labels]));
  for (const labels of sets.values()) {
    const item = el('span', {}, legend);
    el('i', {}, item).style.background = labelColor(labels, COLORS);
    item.append(labelText(labels));
  }
  if (note) el('span', { class: 'graph-note', text: note }, legend);
  el('span', { class: 'graph-hint', text: 'Double-click a node to expand it' }, legend);
}

/**
 * The wheel scrolls the page past a graph until the reader clicks into it
 * (or holds Ctrl/⌘, as a trackpad pinch does); then it zooms, until the
 * pointer leaves. `hint()` runs when a wheel scrolls past.
 */
function gateWheel(root, stage, hint) {
  let engaged = false;
  root.addEventListener('pointerdown', () => (engaged = true));
  root.addEventListener('pointerleave', () => (engaged = false));
  root.addEventListener(
    'wheel',
    (e) => {
      if (!stage.contains(e.target)) return;
      if (engaged || e.ctrlKey || e.metaKey) {
        e.preventDefault(); // zoom the graph, not the page
        return;
      }
      e.stopPropagation(); // keep it from NVL: the page scrolls
      hint();
    },
    { capture: true, passive: false },
  );
}

/** Watch the page's theme: `onChange()` on each change. Returns the function that stops. */
function watchTheme(onChange) {
  const attr = new MutationObserver(onChange);
  attr.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  const media = matchMedia('(prefers-color-scheme: dark)');
  media.addEventListener('change', onChange);
  return () => {
    attr.disconnect();
    media.removeEventListener('change', onChange);
  };
}

// ---------------------------------------------------------------- view

/**
 * Draw `graph` (a recording's `{nodes, rels}`) into `container`. A
 * double-click on a node calls `onExpand(id, drawn)`, `drawn` being the
 * engine ids of the nodes on screen; the answer comes back through the
 * view's `grow` (a run can replace the view in between, and the answer goes
 * to whichever view shows the step then). Returns the view:
 * `{grow(id, outcome), destroy()}`.
 */
export function renderGraph(container, graph, { onExpand }) {
  const original = new DrawnGraph();
  original.add(graph, MAX_NODES);
  const drawn = new DrawnGraph();
  const first = drawn.add(graph, MAX_NODES);
  const note = first.left > 0 ? `first ${MAX_NODES} of ${graph.nodes.length} nodes` : null;
  let theme = canvasTheme();

  const root = el('div', { class: 'graph-view' }, container);
  // NVL sizes its element to its parent's height, so it gets one inside the
  // stage, which the reader can resize.
  const stage = el('div', { class: 'graph-stage' }, root);
  const host = el('div', { class: 'graph-host' }, stage);
  const camera = createCamera(stage, (animated) =>
    nvl.fit([...drawn.nodes.keys()].map(nodeKey), { animated, maxZoom: 1.25 }),
  );
  const nvl = createNvl(host, first, theme, { onLayoutDone: () => camera.settled() });
  const resetBtn = toolbar(root, {
    zoomBy: (factor) => {
      camera.moved();
      nvl.setZoom(nvl.getScale() * factor);
    },
    fit: () => camera.fit(),
    reset: showResult,
  });
  const inspector = createInspector(root, (key) => drawn.get(key), () => nvl.deselectAll());
  const status = statusLine(root);
  const legend = el('div', { class: 'graph-legend' }, root);

  const handlers = interactions(nvl, {
    onMove: () => camera.moved(),
    onPoint: (key) => {
      stage.style.cursor = key ? 'pointer' : '';
      inspector.hover(key);
    },
    onPin: (key) => inspector.pin(key),
    onExpand: expand,
  });
  gateWheel(root, stage, () =>
    status.say('Click the graph (or hold Ctrl/⌘) to zoom with the wheel', 2500),
  );
  const resized = new ResizeObserver(() => camera.resized());
  resized.observe(stage);
  // NVL paints on a canvas, so a theme change repaints relationships.
  const unwatchTheme = watchTheme(() => {
    theme = canvasTheme();
    nvl.updateElementsInGraph([], [...drawn.rels.keys()].map((id) => ({ id: relKey(id), color: theme.rel })));
  });

  let expanding = false;
  async function expand(id) {
    if (expanding) return;
    expanding = true;
    status.say('Expanding…', null);
    try {
      await onExpand(id, [...drawn.nodes.keys()]);
    } finally {
      expanding = false;
    }
  }

  /** Drop every expansion: the step's result again. */
  function showResult() {
    const gone = drawn.keepOnly(original);
    nvl.removeRelationshipsWithIds(gone.rels.map(relKey));
    nvl.removeNodesWithIds(gone.nodes.map(nodeKey));
    inspector.prune();
    resetBtn.hidden = true;
    status.say('');
    drawLegend(legend, drawn, note);
    camera.fit();
  }

  drawLegend(legend, drawn, note);

  return {
    /** Add node `id`'s neighbours (`outcome`: `{graph}` or `{problem}`). */
    grow(id, outcome) {
      const { say } = status;
      if (outcome.problem) return say(`Could not expand: ${outcome.problem}`);
      if (!drawn.nodes.has(id)) return say('That node is not in this result any more.');
      const { nodes, rels, left } = drawn.add(outcome.graph, MAX_DRAWN);
      if (nodes.length === 0 && rels.length === 0) {
        return say(left ? `The view is full (${MAX_DRAWN} nodes).` : 'Nothing to add: every neighbour is drawn.');
      }
      // New nodes start around the expanded one; the layout takes it from there.
      const at = nvl.getPositionById(nodeKey(id)) ?? { x: 0, y: 0 };
      const spread = 40 + 6 * Math.sqrt(nodes.length);
      nvl.addAndUpdateElementsInGraph(
        nodes.map((n, i) => {
          const a = (2 * Math.PI * i) / nodes.length;
          return { ...nvlNode(n, COLORS), x: at.x + spread * Math.cos(a), y: at.y + spread * Math.sin(a) };
        }),
        rels.map((r) => nvlRel(r, theme.rel)),
      );
      resetBtn.hidden = false;
      drawLegend(legend, drawn, note);
      const notes = [];
      if (outcome.graph.truncated) notes.push(`its first ${EXPAND_LIMIT} neighbours`);
      if (left) notes.push(`the view is full at ${MAX_DRAWN} nodes`);
      const added = `Added ${plural(nodes.length, 'node')} and ${plural(rels.length, 'relationship')}`;
      say(`${added}${notes.length ? ` (${notes.join('; ')})` : ''}.`);
    },

    destroy() {
      status.stop();
      resized.disconnect();
      unwatchTheme();
      for (const h of handlers) h.destroy();
      nvl.destroy();
      root.remove();
    },
  };
}
