// Recipe diagrams (ADR-0135): ```mermaid blocks in the recipe (the recipe
// parser lists them), rendered by diagrams/render.mjs to light and dark SVG
// files that the website build inlines, plus, for a data model, the colour
// of each label it draws (a labels file), which the result graph view uses.
// Shared by the renderer and build.mjs.

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { parseFragment, serialize } from 'parse5';

export const THEMES = ['light', 'dark'];

/** SHA-256 of a diagram's source, ignoring trailing whitespace. */
export function sourceHash(source) {
  return createHash('sha256').update(source.trimEnd()).digest('hex');
}

/** The command that brings every rendering up to date. */
export const RENDER_COMMAND = 'npm ci --prefix scripts/cookbook/diagrams && node scripts/cookbook/diagrams/render.mjs';

/** The file a diagram renders to, relative to the diagrams directory. */
export const diagramFile = (slug, index, theme) => `${slug}-${index}-${theme}.svg`;

/** The file a data model's label colours go to, beside its renderings. */
export const labelsFile = (slug, index) => `${slug}-${index}-labels.json`;

/** Every file a diagram can render to: an SVG per theme, and its labels. */
export const diagramFiles = (slug, index) => [...THEMES.map((t) => diagramFile(slug, index, t)), labelsFile(slug, index)];

/** Whether `name`, in the diagrams directory, is a file of a diagram of `slug`. */
export function isDiagramFileOf(slug, name) {
  const index = name.startsWith(`${slug}-`) ? /^(\d+)-/.exec(name.slice(slug.length + 1))?.[1] : undefined;
  return index !== undefined && diagramFiles(slug, Number(index)).includes(name);
}

/** The comment that opens a rendered file and records its source. */
export const hashComment = (hash) => `<!-- mermaid source sha256:${hash} -->`;

// The text of the rendered file at `path` after its hash comment, or null
// when the comment records another source than `diagram`'s.
function renderedBody(path, diagram) {
  const text = readFileSync(path, 'utf8');
  const head = `${hashComment(sourceHash(diagram.source))}\n`;
  return text.startsWith(head) ? text.slice(head.length) : null;
}

/** Whether a diagram's `theme` rendering in `dir` is there and from its current source. */
export function isFresh(dir, slug, diagram, theme) {
  const path = join(dir, diagramFile(slug, diagram.index, theme));
  return existsSync(path) && renderedBody(path, diagram) !== null;
}

/** The name a diagram gives itself with `accTitle:`, or null. */
export function diagramTitle(source) {
  return /^\s*accTitle:\s*(.+?)\s*$/m.exec(source)?.[1] ?? null;
}

// A rendering is inlined into the page as is, and its source hash only ties
// the file to its mermaid block, not its body to what mermaid drew. So the
// build parses it the way the browser will (parse5 is a spec-compliant HTML
// parser) and accepts only the elements, attributes, and CSS that mermaid
// emits: nothing can run, fetch, or navigate.
const SVG_ELEMENTS = new Set([
  'svg', 'g', 'defs', 'style', 'title', 'desc', 'marker', 'filter', 'fedropshadow',
  'path', 'rect', 'circle', 'ellipse', 'line', 'polygon', 'polyline', 'text', 'tspan', 'foreignobject',
]);
// HTML inside a foreignObject label.
const HTML_ELEMENTS = new Set(['div', 'span', 'p', 'br', 'b', 'i', 'em', 'strong', 'small', 'code']);
const ATTRIBUTES = new Set([
  'id', 'class', 'style', 'role', 'xmlns', 'viewbox', 'transform', 'transform-origin',
  'd', 'x', 'y', 'x1', 'x2', 'y1', 'y2', 'cx', 'cy', 'r', 'rx', 'ry', 'dx', 'dy', 'width', 'height', 'points',
  'fill', 'fill-opacity', 'stroke', 'stroke-width', 'stroke-dasharray', 'stroke-opacity', 'opacity',
  'font-family', 'font-size', 'font-weight', 'text-anchor', 'dominant-baseline', 'alignment-baseline',
  'marker-start', 'marker-mid', 'marker-end', 'markerwidth', 'markerheight', 'markerunits', 'orient', 'refx', 'refy',
  'stddeviation', 'flood-color', 'flood-opacity',
]);
const CSS_FUNCTIONS = new Set(['rgb', 'rgba', 'hsl', 'hsla', 'var', 'calc', 'drop-shadow', 'rotate', 'scale', 'translate', 'url']);

/**
 * Why `css` could fetch something, or null: escapes, @-rules other than
 * mermaid's @keyframes, and functions other than CSS_FUNCTIONS are out.
 */
function cssProblem(css) {
  if (css.includes('\\')) return 'a CSS escape';
  if (/@(?!keyframes\b)/i.test(css)) return 'a CSS @-rule';
  for (const m of css.matchAll(/([a-z-]+)\s*\(/gi)) {
    if (!CSS_FUNCTIONS.has(m[1].toLowerCase())) return `the CSS function ${m[1]}()`;
  }
  if (/url\(\s*(?!['"]?#)/i.test(css)) return 'a url() that is not a fragment';
  return null;
}

function nodeProblem(node) {
  if (node.nodeName === '#text' || node.nodeName === '#comment') {
    return node.nodeName === '#text' && node.parentNode?.tagName === 'style' ? cssProblem(node.value) : null;
  }
  const html = node.namespaceURI === 'http://www.w3.org/1999/xhtml';
  if (!(html ? HTML_ELEMENTS : SVG_ELEMENTS).has(node.tagName.toLowerCase())) return `the element <${node.tagName}>`;
  for (const { prefix, name: local, value } of node.attrs) {
    const name = prefix ? `${prefix}:${local}` : local;
    const n = name.toLowerCase();
    if (!ATTRIBUTES.has(n) && !/^(aria|data)-[a-z0-9-]+$/.test(n)) return `the attribute ${name}`;
    if (n === 'style' || /url\(/i.test(value)) {
      const why = cssProblem(value);
      if (why) return `${why} in ${name}`;
    }
  }
  for (const child of node.childNodes ?? []) {
    const why = nodeProblem(child);
    if (why) return why;
  }
  return null;
}

// The page puts each rendering inside a <div>; parse it in that context, as
// the browser will.
const PAGE_CONTEXT = parseFragment('<div></div>').childNodes[0];

/**
 * `svg` as the page will inline it: parsed as the browser will parse it,
 * checked (see above), and serialized from the checked tree, so what ships
 * is exactly what was checked. Throws, naming `where`, unless it is one
 * inert <svg> element.
 */
export function inertSvg(svg, where) {
  const fragment = parseFragment(PAGE_CONTEXT, svg);
  const nodes = fragment.childNodes.filter((n) => !(n.nodeName === '#text' && !n.value.trim()) && n.nodeName !== '#comment');
  if (nodes.length !== 1 || nodes[0].tagName !== 'svg') throw new Error(`${where} is not a single <svg> element`);
  const why = nodeProblem(nodes[0]);
  if (why) throw new Error(`${where} holds ${why}; renderings must be inert`);
  fragment.childNodes = [nodes[0]];
  return serialize(fragment);
}

/**
 * The rendered SVGs of a recipe's diagram ({index, source}), by theme, read
 * from `dir`. Throws when a file is missing, was rendered from a different
 * source, or holds anything active; each is returned as `inertSvg` gives it.
 */
export function readRendered(dir, slug, diagram) {
  const out = {};
  for (const theme of THEMES) {
    const path = join(dir, diagramFile(slug, diagram.index, theme));
    if (!existsSync(path)) {
      throw new Error(`diagram ${diagram.index} of ${slug} has no ${theme} rendering (${path})`);
    }
    const body = renderedBody(path, diagram);
    if (body === null) {
      throw new Error(`diagram ${diagram.index} of ${slug} changed since it was rendered (${path})`);
    }
    out[theme] = inertSvg(body, path);
  }
  return out;
}

/**
 * Write the label colours of a data model (`labels`, label → #RRGGBB) to
 * `dir`, with the hash of the source they were drawn from.
 */
export function writeLabels(dir, slug, diagram, labels) {
  const record = { source: sourceHash(diagram.source), labels };
  writeFileSync(join(dir, labelsFile(slug, diagram.index)), `${JSON.stringify(record, null, 2)}\n`);
}

/**
 * The label colours of a recipe's diagram ({index, source}) read from `dir`:
 * {label: "#RRGGBB"}, the label as the diagram names it; null when it has
 * no labels file (it is not a data model). Throws when the file was written
 * for a different source or does not map labels to colours.
 */
export function readLabels(dir, slug, diagram) {
  const path = join(dir, labelsFile(slug, diagram.index));
  if (!existsSync(path)) return null;
  let record;
  try {
    record = JSON.parse(readFileSync(path, 'utf8'));
  } catch (e) {
    throw new Error(`${path} is not JSON: ${e.message}`);
  }
  if (record?.source !== sourceHash(diagram.source)) {
    throw new Error(`diagram ${diagram.index} of ${slug} changed since its labels were rendered (${path})`);
  }
  const { labels } = record;
  const colours = labels && typeof labels === 'object' && !Array.isArray(labels) ? Object.values(labels) : null;
  if (!colours?.every((c) => typeof c === 'string' && /^#[0-9A-Fa-f]{6}$/.test(c))) {
    throw new Error(`${path} does not map labels to #RRGGBB colours`);
  }
  return labels;
}

/**
 * Prepare a rendered SVG for the page: record its natural width (mermaid's
 * `max-width`) as `--natural-width`, so narrow screens can draw it at a
 * readable size and scroll instead of shrinking it.
 */
export function prepareSvg(svg) {
  return svg.replace(/^(<svg\b[^>]*?style=")max-width: ([\d.]+)px;/, (_, head, w) => `${head}max-width: ${w}px; --natural-width: ${w}px;`);
}
