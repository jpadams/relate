#!/usr/bin/env node
// Render the cookbook's ```mermaid blocks to SVG for the website (ADR-0135).
//
// Each block of docs/cookbook/<slug>.md becomes <slug>-<n>-light.svg and
// <slug>-<n>-dark.svg in docs/cookbook/diagrams/, in the site's palette and
// font. A data model (a flowchart whose nodes are all circles) is drawn as a
// graph by lib/schema-graph.mjs, from mermaid's parse of the block and text
// measured in headless Chrome, and its label colours go to
// <slug>-<n>-labels.json for the result graph view; any other block is drawn
// by mermaid there. The first line of each SVG (the `source` of each labels
// file) records the SHA-256 of its block; build.mjs fails when they differ.
// Files already rendered from the same source are left alone.
//
// An authoring tool: `npm ci` here and in scripts/cookbook, then run it after
// changing a diagram. It reads the recipes through the cookbook harness
// (`cargo`), needs Chrome (CHROME_PATH, or a usual install location), and a
// network connection for the site's font.
//
// Usage: node render.mjs [--cookbook DIR] [--only SLUG[,SLUG…]] [--force]

import { existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import puppeteer from 'puppeteer-core';

import {
  diagramFile,
  diagramFiles,
  diagramTitle,
  hashComment,
  inertSvg,
  isDiagramFileOf,
  isFresh,
  labelsFile,
  readLabels,
  sourceHash,
  THEMES,
  writeLabels,
} from '../lib/diagrams.mjs';
import { LABEL_TEXT, PALETTE, PALETTE_BY_NAME } from '../lib/palette.mjs';
import { loadCookbook } from '../lib/recipes.mjs';
import { drawSchema, FONTS, fontSpec, isSchema, layoutSchema, schemaLabels, schemaModel } from '../lib/schema-graph.mjs';
import { TOKENS } from '../lib/tokens.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const { values: args } = parseArgs({
  options: {
    cookbook: { type: 'string', default: resolve(here, '../../../docs/cookbook') },
    only: { type: 'string' },
    force: { type: 'boolean', default: false },
  },
});
const cookbook = resolve(args.cookbook);
const outDir = join(cookbook, 'diagrams');
const only = args.only ? args.only.split(',').map((s) => s.trim()) : null;

// Mermaid's theme, from the site's tokens (lib/tokens.mjs). Where the themes
// differ beyond their tokens: nodes sit on a raised fill (the alt background
// in light; in dark, the code blocks' grey, a step above it), the secondary
// fill is a step off the page, and dark draws lines and rules a step stronger
// than light.
const SECONDARY_LIGHT = '#EEF2F6';
const THEME_CHOICES = {
  light: { node: TOKENS.light.bgAlt, secondary: SECONDARY_LIGHT, line: TOKENS.light.fgFaint, rule: TOKENS.light.border },
  dark: { node: TOKENS.dark.codeBg, secondary: TOKENS.dark.bgAlt, line: TOKENS.dark.fgMuted, rule: TOKENS.dark.borderStrong },
};
function themeVariables(theme) {
  const t = TOKENS[theme];
  const c = THEME_CHOICES[theme];
  return {
    darkMode: theme === 'dark',
    background: t.bg,
    primaryColor: c.node,
    primaryTextColor: t.fg,
    primaryBorderColor: t.borderStrong,
    secondaryColor: c.secondary,
    tertiaryColor: t.bgAlt,
    lineColor: c.line,
    textColor: t.fg,
    titleColor: t.fg,
    nodeTextColor: t.fg,
    mainBkg: c.node,
    nodeBorder: t.borderStrong,
    clusterBkg: t.bgAlt,
    clusterBorder: c.rule,
    edgeLabelBackground: t.bg,
    // gantt
    gridColor: c.rule,
    sectionBkgColor: t.bgAlt,
    sectionBkgColor2: t.bgAlt,
    altSectionBkgColor: t.bg,
    taskTextOutsideColor: t.fg,
  };
}

// Label classes l1…l12, for mermaid-drawn flowcharts too: the graph view's
// palette (lib/palette.mjs), which reads with dark text on either theme.
const labelClasses = PALETTE.map((c, i) => `  classDef l${i + 1} fill:${c},stroke:${c},color:${LABEL_TEXT}`).join('\n');

// Flow classes: `accent` marks the step a diagram is about; `good` and `bad`
// mark outcomes. Each is outlined in its site token over a tint of it; the
// tints are the diagrams' own.
const FLOW_CLASSES = { accent: 'accent', good: 'success', bad: 'danger' };
const TINTS = {
  light: { accent: '#E6F0FB', success: '#E7F4EA', danger: '#FBEAE5' },
  dark: { accent: '#0E2A44', success: '#11301C', danger: '#3A1A12' },
};
const flowClasses = (theme) =>
  Object.entries(FLOW_CLASSES)
    .map(([name, token]) => {
      const t = TOKENS[theme];
      return `  classDef ${name} fill:${TINTS[theme][token]},stroke:${t[token]},color:${t.fg}`;
    })
    .join('\n');

// Gantt bars from the palette, with dark text: tasks blue, closed (done)
// beige, current (active) green, rejected (crit) red.
const { blue, beige, green, red } = PALETTE_BY_NAME;
const GANTT = {
  taskBkgColor: blue,
  taskBorderColor: blue,
  taskTextColor: LABEL_TEXT,
  taskTextLightColor: LABEL_TEXT,
  taskTextDarkColor: LABEL_TEXT,
  taskTextClickableColor: LABEL_TEXT,
  doneTaskBkgColor: beige,
  doneTaskBorderColor: beige,
  activeTaskBkgColor: green,
  activeTaskBorderColor: green,
  critBkgColor: red,
  critBorderColor: red,
};

// What mermaid draws for a block of diagram type `type`: flowcharts get the
// label and flow classes; gantt charts lose the marker for today, which
// would move the drawing every day it is rendered.
function mermaidSource(source, type, theme) {
  if (type.startsWith('flowchart')) return `${source}\n${labelClasses}\n${flowClasses(theme)}`;
  if (type === 'gantt') return `${source}\ntodayMarker off`;
  return source;
}

function chromePath() {
  const candidates = [
    process.env.CHROME_PATH,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
  ].filter(Boolean);
  const found = candidates.find((p) => existsSync(p));
  if (!found) throw new Error('Chrome not found; set CHROME_PATH');
  return found;
}

// Whether a diagram's files are current: both renderings, and its labels
// file when it has one.
function isCurrent(slug, diagram) {
  if (!THEMES.every((theme) => isFresh(outDir, slug, diagram, theme))) return false;
  try {
    readLabels(outDir, slug, diagram);
    return true;
  } catch {
    return false; // stale or unreadable: render it again
  }
}

const remove = (file) => {
  rmSync(join(outDir, file));
  console.log(`removed ${file}`);
};

// The cookbook harness lists each recipe's mermaid blocks (the same parse the
// build uses, so both agree on what a diagram is).
const recipes = loadCookbook(cookbook, { validate: false }).recipes.filter((r) => !only || only.includes(r.slug));

mkdirSync(outDir, { recursive: true });
const todo = [];
for (const r of recipes) {
  for (const diagram of r.diagrams) {
    if (args.force || !isCurrent(r.slug, diagram)) todo.push({ slug: r.slug, diagram });
  }
  // Files of blocks the recipe no longer has.
  const keep = new Set(r.diagrams.flatMap((d) => diagramFiles(r.slug, d.index)));
  for (const f of readdirSync(outDir)) if (isDiagramFileOf(r.slug, f) && !keep.has(f)) remove(f);
}
if (todo.length === 0) {
  console.log('diagrams: every rendering is current');
  process.exit(0);
}

const require = createRequire(import.meta.url);
const browser = await puppeteer.launch({ executablePath: chromePath(), headless: true });
try {
  const page = await browser.newPage();
  await page.setContent(
    '<!doctype html><html><head><link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Public+Sans:wght@400;500;600&display=block"></head><body></body></html>',
    { waitUntil: 'networkidle0' },
  );
  await page.addScriptTag({ path: require.resolve('mermaid/dist/mermaid.min.js') });
  const fonts = await page.evaluate(async () => {
    await Promise.all(['400', '500', '600'].map((w) => document.fonts.load(`${w} 14px "Public Sans"`)));
    return document.fonts.check('14px "Public Sans"');
  });
  if (!fonts) console.warn('warning: Public Sans did not load; labels are measured with a fallback font');

  // Data models are measured with each character's advance in the site's
  // fonts (kerning is ignored, which errs a little wide).
  const printable = Array.from({ length: 95 }, (_, i) => String.fromCharCode(32 + i));
  const chars = [...new Set([...printable, ...todo.map((t) => t.diagram.source).join('')])];
  const advances = await page.evaluate(
    (specs, chars) => {
      const ctx = document.createElement('canvas').getContext('2d');
      return Object.fromEntries(
        specs.map((spec) => {
          ctx.font = spec;
          return [spec, Object.fromEntries(chars.map((c) => [c, ctx.measureText(c).width]))];
        }),
      );
    },
    Object.values(FONTS).map(fontSpec),
    chars,
  );
  const measure = (text, font) => {
    const table = advances[fontSpec(font)];
    return [...text].reduce((w, c) => w + (table[c] ?? font.size * 0.6), 0);
  };

  // Mermaid's own parse of a block: its diagram type and, for a flowchart,
  // its graph.
  const parse = (source) =>
    page.evaluate(async (source) => {
      window.mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' });
      const diagram = await window.mermaid.mermaidAPI.getDiagramFromText(source);
      if (!diagram.type.startsWith('flowchart')) return { type: diagram.type, flowchart: null };
      const db = diagram.db;
      return {
        type: diagram.type,
        flowchart: {
          direction: db.getDirection(),
          subgraphs: db.getSubGraphs().length,
          vertices: [...db.getVertices().values()].map((v) => ({ id: v.id, text: v.text, type: v.type, classes: v.classes })),
          edges: db.getEdges().map((e) => ({ start: e.start, end: e.end, text: e.text, stroke: e.stroke, type: e.type })),
        },
      };
    }, source);

  // A block drawn by mermaid, in the site's theme.
  const renderMermaid = async (id, source, theme) => {
    const variables = themeVariables(theme);
    const config = {
      startOnLoad: false,
      securityLevel: 'strict',
      theme: 'base',
      fontFamily: '"Public Sans", system-ui, sans-serif',
      themeVariables: { ...variables, ...GANTT, fontFamily: '"Public Sans", system-ui, sans-serif', fontSize: '14px' },
      flowchart: { curve: 'basis', padding: 12, nodeSpacing: 40, rankSpacing: 52, wrappingWidth: 280 },
      gantt: { barHeight: 24, barGap: 8, topPadding: 40, sectionFontSize: 14, fontSize: 13 },
    };
    const svg = await page.evaluate(
      async (id, source, config) => {
        window.mermaid.initialize(config);
        const { svg } = await window.mermaid.render(id, source);
        return svg;
      },
      id,
      source,
      config,
    );
    // Gantt grid lines are drawn with stroke="currentColor", which would take
    // the page's text colour; give them the theme's grid colour.
    const out = svg.replaceAll('<line stroke="currentColor"', `<line stroke="${variables.gridColor}"`);
    // Mermaid draws rounded shapes as long point lists at full precision;
    // two decimals is far below a pixel and cuts the files by most of their
    // size.
    return out.replace(/ d="([^"]*)"/g, (_, d) => ` d="${d.replace(/-?\d+\.\d{3,}/g, (n) => String(Math.round(Number(n) * 100) / 100))}"`);
  };

  for (const { slug, diagram } of todo) {
    const name = `${slug}-${diagram.index}`;
    const parsed = await parse(diagram.source);
    const model = parsed.flowchart && isSchema(parsed.flowchart) ? schemaModel(parsed.flowchart) : null;
    const layout = model && layoutSchema(model, measure);
    for (const p of layout?.problems ?? []) console.warn(`warning: ${name}: ${p}`);
    for (const theme of THEMES) {
      const id = `d-${name}-${theme}`;
      const svg = layout
        ? drawSchema(layout, { id, theme, title: diagramTitle(diagram.source) ?? '' })
        : await renderMermaid(id, mermaidSource(diagram.source, parsed.type, theme), theme);
      // The build accepts only what the renderers are known to draw; fail
      // here, where the cause is visible, if a mermaid upgrade draws
      // something else.
      const file = diagramFile(slug, diagram.index, theme);
      inertSvg(svg, file);
      writeFileSync(join(outDir, file), `${hashComment(sourceHash(diagram.source))}\n${svg}\n`);
      console.log(`${layout ? 'drew' : 'rendered'} ${file}`);
    }
    if (model) {
      writeLabels(outDir, slug, diagram, schemaLabels(model));
      console.log(`wrote ${labelsFile(slug, diagram.index)}`);
    } else if (existsSync(join(outDir, labelsFile(slug, diagram.index)))) {
      remove(labelsFile(slug, diagram.index));
    }
  }
} finally {
  await browser.close();
}
