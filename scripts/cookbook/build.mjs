#!/usr/bin/env node
// Builds the cookbook website from docs/cookbook (ADR-0130, ADR-0132).
//
// Output (--out):
//   index.html                  recipe index, grouped by category
//   <slug>/index.html           recipe page: prose, cells, recorded results
//   <slug>/index.md             the recipe file, for agents and llms.txt
//   <slug>/recipe.json          cells + parameter paths, for playground links
//   <slug>/results.json         each cell's recorded `{envelope, graph}`, fetched
//                               when a graph or JSON view needs it
//   <slug>/data/<name>.json     each `json data=` block, fetched when a cell runs
//   llms.txt                    agent-readable index of the recipes
//   assets/                     page runtime (bundled), worker, graph view, CSS
//   wasm/                       relate.js, relate_bg.wasm, trap.js, and the
//                               snippets/ the glue imports (--wasm-dir)
//
// The build reads the recipes as the Rust harness parses them
// (lib/recipes.mjs), replays each through the WASM bundle and fails if any
// result differs from its recording (the WASM parity check) or the graph
// view cannot expand a node of a graph result (ADR-0151), reads each
// diagram's renderings, renders every page into memory, and fails on a link
// that does not resolve (ADR-0133) or a script import of a file it did not
// write. Only then does it replace --out, so a failed build leaves the last
// good one in place. Pages are complete without JavaScript: every cell shows
// its recorded result.
//
// Usage: node build.mjs [--out DIR] [--wasm-dir DIR] [--cookbook DIR]
//                       [--ref REF] [--site-url URL] [--repo-url URL]
//                       [--lychee FILE] [--only SLUG[,SLUG…]]

import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { gzipSync } from 'node:zlib';

import * as esbuild from 'esbuild';
import { createHighlighter } from 'shiki';

import { diagramTitle, readLabels, readRendered, RENDER_COMMAND } from './lib/diagrams.mjs';
import { checkImports, checkLinks, linkPolicy, rewriteHref } from './lib/links.mjs';
import { createMarkdown } from './lib/markdown.mjs';
import { loadCookbook } from './lib/recipes.mjs';
import { lastEnvelope } from './lib/result-view.mjs';
import { dataPath, indexPage, llmsTxt, mb, recipeJson, recipePage } from './lib/templates.mjs';
import { tokensCss } from './lib/tokens.mjs';
import { expandProblem, loadEngine, parityProblems, runRecipe } from './lib/wasm-run.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const { values: args } = parseArgs({
  options: {
    out: { type: 'string', default: join(here, 'dist') },
    'wasm-dir': { type: 'string', default: join(repo, 'surfaces/relate-wasm/pkg') },
    cookbook: { type: 'string', default: join(repo, 'docs/cookbook') },
    ref: { type: 'string', default: 'main' },
    'site-url': { type: 'string', default: 'https://relatedb.ai/' },
    'repo-url': { type: 'string', default: 'https://github.com/relatedb/relate' },
    lychee: { type: 'string', default: join(repo, 'lychee.toml') },
    only: { type: 'string' },
  },
});
const out = resolve(args.out);
const cookbookDir = resolve(args.cookbook);
const wasmDir = resolve(args['wasm-dir']);
const siteUrl = args['site-url'].replace(/\/?$/, '/');
const only = args.only ? args.only.split(',').map((x) => x.trim()) : null;

function fail(msg) {
  console.error(`build.mjs: ${msg}`);
  process.exit(1);
}

// Which URLs are not public yet: lychee.toml's exclude list (ADR-0133).
let links;
try {
  links = linkPolicy(readFileSync(resolve(args.lychee), 'utf8'));
} catch (e) {
  fail(`${args.lychee}: ${e.message}`);
}

// ---------------------------------------------------------------- phases

/** The recipes, as the Rust harness parses and validates them. */
function loadRecipes() {
  let cookbook;
  try {
    cookbook = loadCookbook(cookbookDir);
  } catch (e) {
    fail(e.message);
  }
  const recipes = cookbook.recipes.filter((r) => !only || only.includes(r.slug));
  if (recipes.length === 0) fail(`no recipes under ${cookbookDir}`);
  return { ...cookbook, recipes };
}

/**
 * Each recipe's cells through the browser build of the engine, checked
 * against the recordings, with the graph view's expansion checked on each
 * graph result (ADR-0151): slug → cell id → `{recording, envelope}`.
 */
async function replay(recipes) {
  if (!existsSync(join(wasmDir, 'relate_bg.wasm'))) {
    fail(`${wasmDir} has no relate_bg.wasm; run scripts/build-wasm.sh first (or pass --wasm-dir)`);
  }
  const engine = await loadEngine(wasmDir).catch((e) => fail(e.message));
  const problems = [];
  const results = new Map();
  for (const r of recipes) {
    const afterCell = async (runStatement, cell, recording) => {
      const problem = await expandProblem(r, cell, runStatement, recording);
      if (problem) problems.push(problem);
    };
    const run = await runRecipe(engine, r, { afterCell });
    problems.push(...parityProblems(r, run));
    const cells = new Map();
    for (const { id, envelopes, recording } of run.cells) {
      cells.set(id, { recording, envelope: lastEnvelope(envelopes) });
    }
    results.set(r.slug, cells);
  }
  if (problems.length) fail(`${problems.length} WASM parity or expand problem(s):\n\n${problems.join('\n\n')}`);
  return results;
}

/**
 * Each recipe's diagram renderings (ADR-0135) and its labels' colours:
 * slug → `{diagrams: index → {title, light, dark}, labels}`.
 */
function loadDiagrams(recipes) {
  const dir = join(cookbookDir, 'diagrams');
  const stale = [];
  const out = new Map();
  for (const r of recipes) {
    const diagrams = new Map();
    const labels = {};
    for (const d of r.diagrams) {
      try {
        diagrams.set(d.index, { title: diagramTitle(d.source), ...readRendered(dir, r.slug, d) });
        Object.assign(labels, readLabels(dir, r.slug, d) ?? {});
      } catch (e) {
        stale.push(e.message);
      }
    }
    out.set(r.slug, { diagrams, labels });
  }
  // Every diagram file is generated, so rendering again fixes any of these.
  if (stale.length) fail(`${stale.join('\n  ')}\n  render them: ${RENDER_COMMAND}`);
  return out;
}

// NVL imports its telemetry, Segment Analytics, for its side effects only,
// and the graph view turns telemetry off (ADR-0151). Tree shaking drops the
// module but not the chunks of its own dynamic imports, so it is replaced
// with an empty one.
const noTelemetry = {
  name: 'no-telemetry',
  setup(build) {
    build.onResolve({ filter: /^@segment\/analytics-next$/ }, (a) => ({ path: a.path, namespace: 'no-telemetry' }));
    build.onLoad({ filter: /.*/, namespace: 'no-telemetry' }, () => ({ contents: '', loader: 'js' }));
  },
};

/** The page runtime, the worker, and the graph view, bundled: path → contents. */
async function bundleAssets() {
  const result = await esbuild.build({
    plugins: [noTelemetry],
    entryPoints: [join(here, 'runtime/cookbook.js'), join(here, 'runtime/worker.js')],
    outdir: join(out, 'assets'),
    bundle: true,
    splitting: true,
    format: 'esm',
    minify: true,
    target: ['es2022'],
    chunkNames: 'chunk-[hash]',
    external: ['../wasm/relate.js', '../wasm/trap.js'],
    logLevel: 'warning',
    write: false,
  });
  return result.outputFiles.map((f) => [relative(out, f.path).split('\\').join('/'), f.contents]);
}

/** Replace `out` with `files` (path → contents), all at once. */
function writeSite(files) {
  const staging = `${out}.partial`;
  rmSync(staging, { recursive: true, force: true });
  for (const [path, contents] of files) {
    const file = join(staging, path);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, contents);
  }
  rmSync(out, { recursive: true, force: true });
  renameSync(staging, out);
}

// ---------------------------------------------------------------- build

const { categories, inlineDataBytes, recipes } = loadRecipes();
const results = await replay(recipes);
const art = loadDiagrams(recipes);
const pages = recipes.map((recipe) => ({
  recipe,
  fences: new Map(recipe.fences.map((f) => [f.line, f])),
  cells: results.get(recipe.slug),
  ...art.get(recipe.slug),
}));

const highlighter = await createHighlighter({
  themes: ['github-dark-default'],
  langs: ['cypher', 'python', 'shellscript', 'json', 'javascript', 'rust'],
});
const LANG_ALIASES = { sh: 'shellscript', bash: 'shellscript', shell: 'shellscript', js: 'javascript', py: 'python' };
function highlight(code, lang) {
  const l = LANG_ALIASES[lang] ?? lang;
  const known = highlighter.getLoadedLanguages().includes(l);
  return highlighter.codeToHtml(code, { lang: known ? l : 'text', theme: 'github-dark-default' });
}

const slugs = new Set(recipes.map((r) => r.slug));
const repoUrl = args['repo-url'].replace(/\/$/, '');
const site = {
  siteUrl,
  cookbookUrl: `${siteUrl}cookbook/`,
  repoUrl,
  ref: args.ref,
  wasmGzip: gzipSync(readFileSync(join(wasmDir, 'relate_bg.wasm'))).length,
  categories,
  inlineDataBytes,
  pages,
  links,
  md: createMarkdown({ rewriteHref: (href) => rewriteHref(href, { slugs, repoUrl, ref: args.ref, links }), siteUrl }),
  highlight,
};

const files = new Map();
for (const page of pages) {
  const r = page.recipe;
  files.set(`${r.slug}/index.html`, recipePage(site, page));
  files.set(`${r.slug}/index.md`, readFileSync(join(cookbookDir, r.file), 'utf8'));
  files.set(
    `${r.slug}/results.json`,
    JSON.stringify(
      Object.fromEntries(
        r.cells.map((c) => {
          const { recording, envelope } = page.cells.get(c.id);
          return [c.id, { envelope, graph: recording.graph }];
        }),
      ),
    ),
  );
  files.set(`${r.slug}/recipe.json`, recipeJson(r));
  for (const [name, text] of r.data) files.set(`${r.slug}/${dataPath(name)}`, text + '\n');
}
files.set('index.html', indexPage(site));
files.set('llms.txt', llmsTxt(site));
for (const [path, contents] of await bundleAssets()) files.set(path, contents);
files.set('assets/cookbook.css', readFileSync(join(here, 'runtime/cookbook.css'), 'utf8').replace('/* @tokens */', tokensCss()));
// The glue imports the JS snippets wasm-bindgen copies beside it (the OPFS
// helper, ADR-0145); a module worker missing one fails to load at all.
const snippets = existsSync(join(wasmDir, 'snippets'))
  ? readdirSync(join(wasmDir, 'snippets'), { recursive: true })
      .map((f) => join('snippets', f))
      .filter((f) => statSync(join(wasmDir, f)).isFile())
  : [];
for (const f of ['relate.js', 'relate_bg.wasm', 'trap.js', ...snippets]) {
  files.set(`wasm/${f.split(sep).join('/')}`, readFileSync(join(wasmDir, f)));
}

// Every link the build writes must resolve (ADR-0133); the nav links the
// repository even while it is not public (ADR-0288). So must every script's
// imports: the Node parity check loads the bundle from --wasm-dir, so only
// this check sees a file the site leaves out.
const html = new Map([...files].filter(([p]) => p.endsWith('.html')));
const broken = checkLinks(html, new Set(files.keys()), { siteUrl, links, exempt: new Set([repoUrl]) });
if (broken.length) fail(`${broken.length} broken link(s):\n  ${broken.join('\n  ')}`);
const unresolved = checkImports(files);
if (unresolved.length) fail(`${unresolved.length} unresolved import(s):\n  ${unresolved.join('\n  ')}`);

writeSite(files);
console.log(
  `cookbook: ${recipes.length} recipes, ${recipes.reduce((n, r) => n + r.cells.length, 0)} cells → ${out} ` +
    `(engine ${mb(site.wasmGzip)} gzip, ref ${args.ref})`,
);
