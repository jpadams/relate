// Layout of the data-model drawings (ADR-0135): nodes placed by force from
// starting layers that follow the diagram's direction, several starts tried,
// and the drawing with the fewest flaws kept.

import { forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY } from 'd3-force';

import {
  captionBox,
  circleHitsPolygon,
  extent,
  HEAD,
  linePoints,
  linkGeometry,
  LOOP,
  loopGeometry,
  polygonsOverlap,
  polylinesCross,
} from './schema-geometry.mjs';
import { captionSize, nodeText } from './schema-text.mjs';

const GAP = 14; // between a relationship's caption and the circles it joins
const TICKS = 400;
const VARIANTS = 12;

/**
 * Lay out a data model (`schemaModel`) with `measure(text, font)` giving a
 * text's width in pixels for a font of FONTS. Deterministic. Returns
 * {nodes, edges, problems}: nodes with x, y, r and their text lines, edges
 * with their geometry (`geo`, a cubic curve: see schema-geometry.mjs), and
 * what looks wrong in the result.
 */
export function layoutSchema(model, measure) {
  const nodes = model.nodes.map((n) => ({ ...n, ...nodeText(n, measure) }));
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const edges = model.edges.map((e) => ({
    ...e,
    s: byId.get(e.source),
    t: byId.get(e.target),
    cap: captionSize(e, measure),
    bend: 0,
  }));
  // Relationships between two nodes, and from a node to itself.
  const links = edges.filter((e) => e.s !== e.t);
  const loops = edges.filter((e) => e.s === e.t);

  // Relationships between the same two nodes, either way, bend apart.
  for (const e of links) e.pair = e.s.id < e.t.id ? [e.s, e.t] : [e.t, e.s];
  const pairs = groupBy(links, (e) => `${e.pair[0].id}\n${e.pair[1].id}`).map((group) => {
    const [a, b] = group[0].pair;
    const spacing = Math.max(...group.map((e) => e.cap.h)) + 14;
    group.forEach((e, i) => {
      e.bend = (i - (group.length - 1) / 2) * spacing;
    });
    const span = Math.max(56, ...group.map((e) => e.cap.w + 2 * GAP + HEAD));
    return {
      source: a,
      target: b,
      span,
      distance: a.r + b.r + span,
      clear: Math.max(...group.map((e) => Math.abs(e.bend))) + spacing / 2 + 16,
    };
  });

  // A node's relationships to itself get a body of their own, which the
  // layout keeps clear; the loops point at it.
  const loopBodies = groupBy(loops, (e) => e.s).map((own) => ({
    owner: own[0].s,
    own,
    loop: true,
    r: Math.max(...own.map((e) => Math.max(e.cap.w, e.cap.h))) / 2 + 8,
  }));

  const bodies = [...nodes, ...loopBodies];
  const springs = [
    ...pairs.map((p) => ({ source: p.source, target: p.target, distance: p.distance })),
    ...loopBodies.map((b) => ({ source: b.owner, target: b, distance: b.owner.r + LOOP + b.r })),
  ];
  const rank = ranks(nodes, links);
  const vertical = ['TB', 'TD', 'BT'].includes(model.direction);

  // Force layouts settle where they start; try several starts and keep the
  // drawing with the fewest flaws and crossings, then the smallest.
  let best = null;
  for (let variant = 0; variant < VARIANTS; variant++) {
    seed(nodes, loopBodies, rank, vertical, variant);
    simulate(bodies, springs, pairs);
    orient(nodes, bodies, rank);
    place(links, loopBodies);
    const score = scoreLayout(nodes, edges, links);
    if (!best || score < best.score) best = { score, at: bodies.map((b) => [b.x, b.y]) };
  }
  bodies.forEach((b, i) => {
    [b.x, b.y] = best.at[i];
  });
  place(links, loopBodies);
  return { nodes, edges, problems: problems(nodes, edges, links) };
}

// `items` grouped by `key(item)`, in the order each key first appears.
function groupBy(items, key) {
  const groups = new Map();
  for (const item of items) {
    const k = key(item);
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(item);
  }
  return [...groups.values()];
}

// Longest-path ranks along the relationships between nodes, ignoring those
// that close a cycle (found depth-first, in source order).
function ranks(nodes, links) {
  const out = new Map(nodes.map((n) => [n, []]));
  for (const e of links) out.get(e.s).push(e.t);
  const state = new Map();
  const back = new Set();
  const order = [];
  const visit = (n) => {
    state.set(n, 'open');
    for (const m of out.get(n)) {
      if (state.get(m) === 'open') back.add(`${n.id}\n${m.id}`);
      else if (!state.has(m)) visit(m);
    }
    state.set(n, 'done');
    order.push(n);
  };
  for (const n of nodes) if (!state.has(n)) visit(n);
  const rank = new Map(nodes.map((n) => [n, 0]));
  for (const n of order.reverse()) {
    for (const m of out.get(n)) {
      if (!back.has(`${n.id}\n${m.id}`)) rank.set(m, Math.max(rank.get(m), rank.get(n) + 1));
    }
  }
  return rank;
}

// Starting positions: layers along the diagram's direction, so the drawing
// reads the way the source does; variant 0 staggers them, the others add
// deterministic noise.
function seed(nodes, loopBodies, rank, vertical, variant) {
  let state = variant * 7919 + 17;
  const noise = () => {
    state = (state * 48271) % 2147483647;
    return state / 2147483647 - 0.5;
  };
  const layers = new Map();
  for (const n of nodes) layers.set(rank.get(n), [...(layers.get(rank.get(n)) ?? []), n]);
  for (const layer of layers.values()) {
    layer.forEach((n, i) => {
      const depth = rank.get(n) * 260 + (variant ? noise() * 140 : 0);
      const side = (i - (layer.length - 1) / 2) * 200 + (variant ? noise() * 260 : (rank.get(n) % 2 ? 50 : -50));
      [n.x, n.y] = vertical ? [side, depth] : [depth, side];
      n.vx = n.vy = 0;
    });
  }
  for (const b of loopBodies) {
    const reach = b.owner.r + LOOP + b.r;
    [b.x, b.y] = vertical ? [b.owner.x + reach, b.owner.y] : [b.owner.x, b.owner.y - reach];
    b.vx = b.vy = 0;
  }
}

function simulate(bodies, springs, pairs) {
  const sim = forceSimulation(bodies)
    .alphaDecay(1 - 0.001 ** (1 / TICKS))
    .force('link', forceLink(springs).distance((l) => l.distance).strength(0.9))
    .force('charge', forceManyBody().strength((b) => (b.loop ? -400 : -900)).distanceMax(900))
    .force('collide', forceCollide((b) => b.r + (b.loop ? 10 : 28)).iterations(2))
    .force('x', forceX(0).strength(0.03))
    .force('y', forceY(0).strength(0.06))
    .force('avoid', forceAvoid(pairs, bodies))
    .stop();
  for (let i = 0; i < TICKS; i++) sim.tick();
}

// Keeps nodes (and loop captions) off the relationships between other nodes,
// and the captions of different pairs apart.
function forceAvoid(pairs, bodies) {
  return (alpha) => {
    for (const p of pairs) {
      const { source: a, target: b } = p;
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const len2 = dx * dx + dy * dy || 1;
      for (const n of bodies) {
        if (n === a || n === b) continue;
        const u = Math.max(0, Math.min(1, ((n.x - a.x) * dx + (n.y - a.y) * dy) / len2));
        let vx = n.x - (a.x + u * dx);
        let vy = n.y - (a.y + u * dy);
        let d = Math.hypot(vx, vy);
        const need = n.r + p.clear;
        if (d >= need) continue;
        if (d < 1e-6) {
          [vx, vy] = [-dy, dx];
          d = Math.sqrt(len2);
        }
        const k = ((need - d) / d) * alpha * 0.5;
        n.vx += vx * k;
        n.vy += vy * k;
        a.vx -= vx * k * (1 - u);
        a.vy -= vy * k * (1 - u);
        b.vx -= vx * k * u;
        b.vy -= vy * k * u;
      }
    }
    for (let i = 0; i < pairs.length; i++) {
      for (let j = i + 1; j < pairs.length; j++) {
        const p = pairs[i];
        const q = pairs[j];
        let vx = (q.source.x + q.target.x - p.source.x - p.target.x) / 2;
        let vy = (q.source.y + q.target.y - p.source.y - p.target.y) / 2;
        const d = Math.hypot(vx, vy) || 1;
        const need = (p.span + q.span) / 2;
        if (d >= need) continue;
        const k = ((need - d) / d) * alpha * 0.25;
        vx *= k;
        vy *= k;
        for (const n of [q.source, q.target]) {
          n.vx += vx;
          n.vy += vy;
        }
        for (const n of [p.source, p.target]) {
          n.vx -= vx;
          n.vy -= vy;
        }
      }
    }
  };
}

// Turn the drawing to lie along its longest axis, first layer on the left.
function orient(nodes, bodies, rank) {
  const cx = nodes.reduce((s, n) => s + n.x, 0) / nodes.length;
  const cy = nodes.reduce((s, n) => s + n.y, 0) / nodes.length;
  let [sxx, syy, sxy] = [0, 0, 0];
  for (const n of nodes) {
    sxx += (n.x - cx) ** 2;
    syy += (n.y - cy) ** 2;
    sxy += (n.x - cx) * (n.y - cy);
  }
  const turn = -0.5 * Math.atan2(2 * sxy, sxx - syy);
  const [cos, sin] = [Math.cos(turn), Math.sin(turn)];
  for (const b of bodies) {
    const [x, y] = [b.x - cx, b.y - cy];
    [b.x, b.y] = [x * cos - y * sin, x * sin + y * cos];
  }
  const last = Math.max(...rank.values());
  const meanX = (r) => {
    const layer = nodes.filter((n) => rank.get(n) === r);
    return layer.reduce((s, n) => s + n.x, 0) / layer.length;
  };
  if (last > 0 && meanX(0) > meanX(last)) for (const b of bodies) b.x = -b.x;
}

// Every relationship's geometry, from where the nodes and loop bodies are.
function place(links, loopBodies) {
  for (const e of links) e.geo = linkGeometry(e);
  for (const { owner, own, x, y } of loopBodies) {
    const theta = Math.atan2(y - owner.y, x - owner.x);
    own.forEach((e, i) => {
      e.geo = loopGeometry(owner, e, theta + (i - (own.length - 1) / 2) * 1.2);
    });
  }
}

// What a reader would see as a flaw: overlapping nodes or captions, a line
// through a node, a caption longer than its line.
function problems(nodes, edges, links) {
  const out = [];
  const name = (e) => `${e.s.id} ${e.type || '→'} ${e.t.id}`;
  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      const [a, b] = [nodes[i], nodes[j]];
      if (Math.hypot(a.x - b.x, a.y - b.y) < a.r + b.r + 8) out.push(`${a.id} and ${b.id} overlap`);
    }
  }
  const boxes = edges.map((e) => ({ e, box: captionBox(e) }));
  for (const { e, box } of boxes) {
    for (const n of nodes) {
      if (circleHitsPolygon(n, n.r, box)) out.push(`the caption of ${name(e)} overlaps ${n.id}`);
    }
    for (const n of nodes) {
      if (n === e.s || n === e.t) continue;
      if (linePoints(e.geo).some((p) => Math.hypot(p.x - n.x, p.y - n.y) < n.r + 2)) out.push(`${name(e)} crosses ${n.id}`);
    }
  }
  for (const e of links) {
    if (e.geo.length < e.cap.w + 2 * GAP) out.push(`${name(e)} is too short for its caption`);
  }
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      if (polygonsOverlap(boxes[i].box, boxes[j].box)) {
        out.push(`the captions of ${name(boxes[i].e)} and ${name(boxes[j].e)} overlap`);
      }
    }
  }
  return out;
}

// Lower is better: flaws, then lines that cross, then area, then height
// beyond what a page column shows well.
function scoreLayout(nodes, edges, links) {
  const lines = edges.map((e) => linePoints(e.geo));
  let crossings = 0;
  for (let i = 0; i < edges.length; i++) {
    for (let j = i + 1; j < edges.length; j++) {
      const [a, b] = [edges[i], edges[j]];
      if ([a.s, a.t].some((n) => n === b.s || n === b.t)) continue;
      if (polylinesCross(lines[i], lines[j])) crossings++;
    }
  }
  const { width, height } = extent(nodes, edges);
  const flaws = problems(nodes, edges, links).length;
  return flaws * 100 + crossings * 30 + (width * height) / 10000 + Math.max(0, height / width - 0.75) * 40;
}
