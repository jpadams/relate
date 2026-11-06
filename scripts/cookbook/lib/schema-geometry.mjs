// Geometry of the data-model drawings (ADR-0135): where a relationship's
// line, arrowhead, and caption go, and the plane geometry the layout checks
// a drawing with. Points are {x, y}.
//
// Every relationship is drawn as one cubic Bézier curve {start, c1, c2,
// end}: a straight line and a quadratic bend are raised to a cubic exactly
// (`toCubic`), so everything downstream handles one shape.

export const HEAD = 10; // arrowhead length
// A relationship from a node to itself: a loop LOOP_FOOT wide where it meets
// the node, flaring LOOP_FLARE radians out on each side, its control points
// LOOP from the node.
export const LOOP = 58;
const LOOP_FOOT = 15;
const LOOP_FLARE = 0.62;

export const unit = (from, to) => {
  const x = to.x - from.x;
  const y = to.y - from.y;
  const l = Math.hypot(x, y) || 1;
  return { x: x / l, y: y / l };
};
export const along = (p, d, k) => ({ x: p.x + d.x * k, y: p.y + d.y * k });
export const polar = (a) => ({ x: Math.cos(a), y: Math.sin(a) });
const lerp = (a, b, t) => ({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t });

/** The point at `t` (0…1) of the Bézier curve on `points`, of any degree. */
export function bezierAt(points, t) {
  let ps = points;
  while (ps.length > 1) ps = ps.slice(1).map((p, i) => lerp(ps[i], p, t));
  return ps[0];
}

/**
 * The line or quadratic Bézier curve on `points` as the same curve of degree
 * three, {start, c1, c2, end} (degree elevation: a quadratic's c1 is 2/3 of
 * the way from its start to its control point; a line's are at its thirds).
 */
export function toCubic(points) {
  let ps = points;
  while (ps.length < 4) {
    const n = ps.length;
    ps = [ps[0], ...ps.slice(1).map((p, i) => lerp(p, ps[i], (i + 1) / n)), ps[n - 1]];
  }
  const [start, c1, c2, end] = ps;
  return { start, c1, c2, end };
}

const cubicPoints = (g) => [g.start, g.c1, g.c2, g.end];

// An arrowhead with its tip at `tip`, pointing along `d`.
function arrowhead(tip, d) {
  const base = along(tip, d, -HEAD);
  const n = { x: -d.y, y: d.x };
  return [tip, along(base, n, HEAD * 0.45), along(base, n, -HEAD * 0.45)];
}

/**
 * A relationship between two nodes (`e.s`, `e.t`): straight, or bent
 * `e.bend` apart from others between the same pair (`e.pair`); its caption
 * at the middle, along it, upright.
 */
export function linkGeometry(e) {
  const { s, t } = e;
  const [a, b] = e.pair;
  const d = unit(a, b);
  const mid = { x: (s.x + t.x) / 2, y: (s.y + t.y) / 2 };
  const control = along(mid, { x: -d.y, y: d.x }, 2 * e.bend);
  const aim = (n, other) => unit(n, e.bend ? control : other);
  const p0 = along(s, aim(s, t), s.r);
  const tip = along(t, aim(t, s), t.r + 1);
  const dir = unit(e.bend ? control : p0, tip);
  const end = e.arrow ? along(tip, dir, -HEAD * 0.7) : along(t, aim(t, s), t.r);
  // The curve on its end points, through `control` when bent.
  const curve = (from, to) => (e.bend ? [from, control, to] : [from, to]);
  const at = bezierAt(curve(p0, tip), 0.5);
  let angle = (Math.atan2(tip.y - p0.y, tip.x - p0.x) * 180) / Math.PI;
  if (angle > 90) angle -= 180;
  else if (angle < -90) angle += 180;
  return {
    ...toCubic(curve(p0, end)),
    head: e.arrow ? arrowhead(tip, dir) : null,
    length: Math.hypot(tip.x - p0.x, tip.y - p0.y),
    caption: { ...at, angle, top: -e.cap.typeH / 2 },
  };
}

/**
 * A relationship from node `n` to itself, leaving it at angle `theta`; its
 * caption level, beyond the loop.
 */
export function loopGeometry(n, e, theta) {
  const half = Math.asin(Math.min(1, LOOP_FOOT / n.r)); // the loop's foot on the circle
  const start = along(n, polar(theta - half), n.r);
  const tip = along(n, polar(theta + half), n.r + 1);
  const c1 = along(start, polar(theta - LOOP_FLARE), LOOP);
  const c2 = along(tip, polar(theta + LOOP_FLARE), LOOP);
  const dir = unit(c2, tip);
  const apex = bezierAt([start, c1, c2, tip], 0.5);
  const out = polar(theta);
  const reach = (Math.abs(out.x) * e.cap.w) / 2 + (Math.abs(out.y) * e.cap.h) / 2 + 4;
  const at = along(apex, out, reach);
  return {
    start,
    c1,
    c2,
    end: e.arrow ? along(tip, dir, -HEAD * 0.7) : tip,
    head: e.arrow ? arrowhead(tip, dir) : null,
    caption: { x: at.x, y: at.y, angle: 0, top: -e.cap.h / 2 },
  };
}

/** The corners of a relationship's caption box. */
export function captionBox(e) {
  const { x, y, angle, top } = e.geo.caption;
  const a = (angle * Math.PI) / 180;
  const [cos, sin] = [Math.cos(a), Math.sin(a)];
  const w = e.cap.w / 2 + 3;
  return [
    [-w, top - 2],
    [w, top - 2],
    [w, top + e.cap.h + 2],
    [-w, top + e.cap.h + 2],
  ].map(([u, v]) => ({ x: x + u * cos - v * sin, y: y + u * sin + v * cos }));
}

/** Whether two convex polygons overlap (separating axes). */
export function polygonsOverlap(p, q) {
  for (const poly of [p, q]) {
    for (let i = 0; i < poly.length; i++) {
      const a = poly[i];
      const b = poly[(i + 1) % poly.length];
      const n = { x: b.y - a.y, y: a.x - b.x };
      const project = (ps) => ps.map((c) => c.x * n.x + c.y * n.y);
      const [pp, qq] = [project(p), project(q)];
      if (Math.max(...pp) <= Math.min(...qq) || Math.max(...qq) <= Math.min(...pp)) return false;
    }
  }
  return true;
}

/** Whether a circle overlaps a convex polygon. */
export function circleHitsPolygon(c, r, poly) {
  const inside = poly.every((a, i) => {
    const b = poly[(i + 1) % poly.length];
    return (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) >= 0;
  });
  if (inside) return true;
  return poly.some((a, i) => distanceToSegment(c, a, poly[(i + 1) % poly.length]) < r);
}

export function distanceToSegment(p, a, b) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const u = Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy || 1)));
  return Math.hypot(p.x - (a.x + u * dx), p.y - (a.y + u * dy));
}

/** Points along a relationship's line (its geometry), for overlap checks. */
export function linePoints(g) {
  return Array.from({ length: 17 }, (_, i) => bezierAt(cubicPoints(g), i / 16));
}

export function segmentsCross(p, q, r, s) {
  const cross = (a, b, c) => (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
  return cross(p, q, r) * cross(p, q, s) < 0 && cross(r, s, p) * cross(r, s, q) < 0;
}

export function polylinesCross(a, b) {
  for (let i = 1; i < a.length; i++) {
    for (let j = 1; j < b.length; j++) if (segmentsCross(a[i - 1], a[i], b[j - 1], b[j])) return true;
  }
  return false;
}

/** A drawing's extent: circles, captions, lines. */
export function extent(nodes, edges) {
  const points = [
    ...nodes.flatMap((n) => [
      { x: n.x - n.r - 1, y: n.y - n.r - 1 },
      { x: n.x + n.r + 1, y: n.y + n.r + 1 },
    ]),
    ...edges.flatMap((e) => [...captionBox(e), ...linePoints(e.geo)]),
  ];
  const minX = Math.min(...points.map((p) => p.x));
  const minY = Math.min(...points.map((p) => p.y));
  return {
    minX,
    minY,
    width: Math.max(...points.map((p) => p.x)) - minX,
    height: Math.max(...points.map((p) => p.y)) - minY,
  };
}
