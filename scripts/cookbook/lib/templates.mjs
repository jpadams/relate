// The cookbook website's pages (ADR-0130): pure functions from the site
// (its settings and renderers) and the pages' models to HTML and text.
//
// A site is `{siteUrl, cookbookUrl, repoUrl, ref, wasmGzip, categories,
// inlineDataBytes, pages, links, md, highlight}`: `pages` is every page
// model, `links` the link policy (lib/links.mjs), `md` comes from
// lib/markdown.mjs, and `highlight(code, lang)` returns HTML. A
// page model is `{recipe, fences, cells, diagrams, labels}`: the recipe as
// cookbook-export parses it, its fences by file line, each cell's replayed
// result (`{recording, envelope}` by cell id), its diagrams' renderings
// (`{title, light, dark}` by index), and its labels' colours for the graph
// view.

import { readFileSync } from 'node:fs';
import { posix } from 'node:path';

import { prepareSvg } from './diagrams.mjs';
import { esc } from './html.mjs';
import { slugify } from './markdown.mjs';
import { canDraw, tableHtml } from './result-view.mjs';

const GLYPH = readFileSync(new URL('../runtime/glyph.svg', import.meta.url), 'utf8').trim();

const EXT_ICON =
  '<svg class="ext-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 17L17 7M9 7h8v8"/></svg>';
// A console window: "open this step in the playground".
const PLAYGROUND_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';

const DEFAULT_LICENSE = 'Apache-2.0';

export const kb = (bytes) => (bytes < 1024 ? `${bytes} B` : `${Math.round(bytes / 1024)} KB`);
export const mb = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;

/** A git ref as a reader sees it: full commit ids shortened to 7 characters. */
export const shortRef = (ref) => (/^[0-9a-f]{40}$/.test(ref) ? ref.slice(0, 7) : ref);

/** Where a data block is published, relative to its recipe page. */
export const dataPath = (name) => `data/${name}.json`;

/** The playground's hash for a recipe, or for one of its steps (ADR-0134). */
export const playgroundHash = (slug, cellId = null) =>
  `#recipe=${encodeURIComponent(slug)}${cellId ? `&cell=${encodeURIComponent(cellId)}` : ''}`;

const byTitle = (a, b) => Buffer.compare(Buffer.from(a.front.title), Buffer.from(b.front.title));

/**
 * `recipes` grouped by category, in `categories` order, each group sorted by
 * title in byte order (as the README index sorts); empty groups left out.
 */
export function byCategory(recipes, categories) {
  return categories
    .map((category) => ({ category, recipes: recipes.filter((r) => r.front.category === category).sort(byTitle) }))
    .filter((g) => g.recipes.length > 0);
}

// ---------------------------------------------------------------- cells

function renderParams(site, recipe, cell, step) {
  if (!cell.params) return '';
  const text = recipe.data.get(cell.params);
  const size = Buffer.byteLength(text);
  if (size <= site.inlineDataBytes) {
    const rows = Math.min(text.split('\n').length + 1, 12);
    return (
      `<details class="cell-params"><summary>Parameters</summary>` +
      `<textarea class="params-editor" rows="${rows}" spellcheck="false" aria-label="Parameters for step ${step} (JSON)">${esc(text)}</textarea></details>`
    );
  }
  const file = dataPath(cell.params);
  return `<div class="cell-params">Loads <a href="${esc(file)}">${esc(posix.basename(file))}</a> (${kb(size)})</div>`;
}

function renderCell(site, page, cell, env) {
  const { recipe } = page;
  const { recording } = page.cells.get(cell.id);
  const step = ++env.step;
  const attrs = [
    `id="cell-${esc(cell.id)}"`,
    `data-cell="${esc(cell.id)}"`,
    `data-step="${step}"`,
    cell.params ? `data-params="${esc(dataPath(cell.params))}"` : '',
    cell.setup ? 'data-setup' : '',
    cell.expectError ? 'data-expect-error' : '',
    `data-result-kind="${cell.result.kind}"`,
    canDraw(recording.graph) ? 'data-has-graph' : '',
  ]
    .filter(Boolean)
    .join(' ');
  // A table result shows as an HTML table; its text form stays in the page
  // (hidden) as the recording the runtime compares with, and for Copy.
  const table = recording.table ? tableHtml(recording.table) : '';
  const body =
    `<div class="cell-code">${site.highlight(cell.code, 'cypher')}</div>` +
    `<textarea class="cell-editor" spellcheck="false" hidden aria-label="Cypher for step ${step}">${esc(cell.code)}</textarea>` +
    renderParams(site, recipe, cell, step) +
    `<div class="cell-output" aria-live="polite">` +
    `<div class="out-head"><span class="out-label">${cell.expectError ? 'Expected error' : 'Recorded result'}</span>` +
    `<button class="out-copy" type="button" data-copy-target="out-${esc(cell.id)}" hidden>Copy as text</button>` +
    `<div class="out-tabs" role="tablist" hidden>` +
    `<button type="button" role="tab" data-view="text" aria-selected="true">Result</button>` +
    `<button type="button" role="tab" data-view="graph" aria-selected="false" hidden>Graph</button>` +
    `<button type="button" role="tab" data-view="json" aria-selected="false">JSON</button></div></div>` +
    `<div class="out-table"${table ? '' : ' hidden'}>${table}</div>` +
    `<pre class="out-text" id="out-${esc(cell.id)}"${table ? ' hidden' : ''}>${esc(cell.result.text)}</pre>` +
    `<div class="out-graph" id="graph-${esc(cell.id)}" hidden></div>` +
    `<pre class="out-json" hidden></pre>` +
    `</div>`;
  const head =
    `<div class="cell-head"><span class="cell-tag">Step ${step}</span>` +
    `<div class="cell-actions">` +
    `<button class="btn primary" type="button" data-action="run" hidden>Run</button>` +
    `<button class="btn" type="button" data-action="edit" hidden>Edit</button>` +
    `<a class="btn icon-only" href="../../playground/${esc(playgroundHash(recipe.slug, cell.id))}" target="_blank" rel="noopener" ` +
    `title="Open this step in the playground" aria-label="Open step ${step} in the playground">${PLAYGROUND_ICON}</a>` +
    `</div></div>`;
  const wrapped = cell.setup ? `<details class="cell-body"><summary>Show the setup step</summary>${body}</details>` : body;
  return `<cypher-cell ${attrs}>${head}${wrapped}</cypher-cell>\n`;
}

/** A diagram: both renderings inline (CSS shows the current theme's), captioned by its accTitle. */
function renderDiagram(index, d) {
  const title = d.title ? esc(d.title) : `Diagram ${index}`;
  return (
    `<figure class="diagram">` +
    `<div class="diagram-art diagram-light">${prepareSvg(d.light)}</div>` +
    `<div class="diagram-art diagram-dark">${prepareSvg(d.dark)}</div>` +
    `<figcaption>${title}</figcaption></figure>\n`
  );
}

/** A fence of the body: `line` is its 0-based line there. */
function renderFence(site, page, token, line, env) {
  const { recipe } = page;
  const fence = page.fences.get(recipe.bodyLine + line);
  if (!fence) {
    throw new Error(
      `[${recipe.slug}] the fence at line ${recipe.bodyLine + line} is not one the recipe parser found; ` +
        'start fences at the beginning of a line',
    );
  }
  switch (fence.kind) {
    case 'cell':
      return renderCell(site, page, recipe.cells.find((c) => c.id === fence.name), env);
    case 'diagram':
      return renderDiagram(fence.name, page.diagrams.get(fence.name));
    // Data blocks are published as files; result blocks fill their cell's pane.
    case 'data':
    case 'result':
      return '';
    default: {
      const id = `code-${++env.codeSeq}`;
      const lang = token.info.trim().split(/\s+/)[0] || 'text';
      return (
        `<div class="code-block">${site.highlight(token.content.replace(/\n$/, ''), lang)}` +
        `<textarea id="${id}" hidden>${esc(token.content)}</textarea>` +
        `<button class="copy-btn" type="button" data-copy-target="${id}">Copy</button></div>\n`
      );
    }
  }
}

// ---------------------------------------------------------------- page shell

function shell(site, { title, description, canonical, depth, body, head = '', footerLinks = '', playground = '' }) {
  const root = '../'.repeat(depth); // site root, relative
  const cb = '../'.repeat(depth - 1); // cookbook root, relative
  const nav = [
    [`${root}#examples`, 'Examples'],
    [`${root}#reference`, 'Reference'],
    [`${root}#quickstart`, 'Quickstart'],
  ]
    .map(([href, label]) => `<a href="${href}">${label}</a>`)
    .concat(
      `<a href="${cb || './'}" aria-current="page">Cookbook</a>`,
      // Linked even while the repository is not public, as the landing page's nav does (ADR-0288).
      `<a href="${esc(site.repoUrl)}" target="_blank" rel="noopener">GitHub</a>`,
    )
    .join('\n      ');
  const github = site.links.publicLink(site.repoUrl, 'GitHub');
  const built =
    site.links.publicLink(`${site.repoUrl}/tree/${site.ref}/docs/cookbook`, esc(shortRef(site.ref))) || esc(shortRef(site.ref));
  const footer = [
    footerLinks,
    github,
    site.links.publicLink(`${site.repoUrl}/blob/${site.ref}/docs/cookbook/README.md`, 'Write a recipe'),
    `<a href="${cb}llms.txt">llms.txt</a>`,
  ]
    .filter(Boolean)
    .join('\n      ');
  return `<!DOCTYPE html>
<html lang="en"><head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>${esc(title)}</title>
<meta name="description" content="${esc(description)}">
<link rel="canonical" href="${esc(canonical)}">
<link rel="icon" type="image/png" href="${root}img/favicon.png">
<link rel="describedby" href="${cb}llms.txt">
<meta property="og:title" content="${esc(title)}">
<meta property="og:description" content="${esc(description)}">
<meta property="og:url" content="${esc(canonical)}">
<meta property="og:type" content="article">
<script>try{const t=localStorage.getItem('theme');if(t==='light'||t==='dark')document.documentElement.setAttribute('data-theme',t)}catch(e){}</script>
<link rel="stylesheet" href="${cb}assets/cookbook.css">
<script type="module" src="${cb}assets/cookbook.js"></script>${head}
</head>
<body>
<header class="top" id="top">
  <div class="container inner">
    <a href="${root}" class="brand">${GLYPH}<span class="name">relate</span><span class="brand-tag">labs</span></a>
    <nav class="top-nav" id="topNav">
      ${nav}
      <a href="${root}playground/${playground}" target="_blank" rel="noopener" class="nav-external"${playground ? ' title="Open this recipe in the playground"' : ''}>Playground${EXT_ICON}</a>
    </nav>
    <button class="icon-btn theme-toggle" id="themeToggle" aria-label="Toggle theme" title="Toggle theme">
      <svg class="sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="4"></circle><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"></path></svg>
      <svg class="moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"></path></svg>
    </button>
    <button class="icon-btn nav-toggle" id="navToggle" aria-label="Open menu" aria-controls="topNav" aria-expanded="false">
      <svg class="open" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M3 12h18M3 18h18"></path></svg>
      <svg class="close" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 6l12 12M6 18L18 6"></path></svg>
    </button>
  </div>
</header>
<main>
${body}
</main>
<footer>
  <div class="container inner">
    <div class="meta">relate cookbook · built from ${built}</div>
    <div class="links">
      ${footer}
    </div>
  </div>
</footer>
</body>
</html>
`;
}

/** A recipe card; `base` is the cookbook root, relative to the page. */
function recipeCard(r, base) {
  return `<li><a class="recipe-card" href="${base}${r.slug}/">
  <span class="title">${esc(r.front.title)}</span>
  <span class="summary">${esc(r.front.summary)}</span>
  <span class="card-meta">${esc(r.front.level)}</span>
</a></li>`;
}

// ---------------------------------------------------------------- pages

/** A recipe's page. */
export function recipePage(site, page) {
  const { recipe } = page;
  const f = recipe.front;
  const env = { step: 0, codeSeq: 0, lede: null, toc: [] };
  env.fence = (token, line) => renderFence(site, page, token, line, env);
  const html = site.md.render(recipe.narrative, env);
  const facts = [
    ['Dataset', f.dataset],
    ['Source', f.source],
    ['License', f.license],
  ]
    .filter(([, v]) => v)
    .map(([k, v]) => `<dt>${k}</dt><dd>${site.md.renderInline(v, env)}</dd>`)
    .join('');
  const program = f.companion
    ? site.links.publicLink(f.companion, `${esc(f.companion.replace(/^https:\/\//, ''))}${EXT_ICON}`)
    : '';
  const companion = program ? `<p>The full program this recipe distils: ${program}</p>` : '';
  const about = facts
    ? `<section class="about-data">
<h2 id="about-the-data">About the data</h2>
<dl class="facts">${facts}</dl>
<p>Every data block is published beside this page as <code>data/&lt;name&gt;.json</code>, and the whole recipe as <a href="index.md">markdown</a>.</p>
${companion}</section>`
    : '';
  const licenseNote =
    f.license && f.license !== DEFAULT_LICENSE
      ? `<p class="data-note">The data is licensed ${site.md.renderInline(f.license, env)}; see <a href="#about-the-data">About the data</a>.</p>`
      : '';
  const toc = [...env.toc, ...(about ? [{ id: 'about-the-data', label: 'About the data' }] : [])];
  const tocNav =
    toc.length >= 3
      ? `<nav class="toc" aria-label="On this page"><details><summary>On this page</summary><ol>${toc
          .map((e) => `<li><a href="#${esc(e.id)}">${e.label}</a></li>`)
          .join('')}</ol></details></nav>`
      : '';
  const siblings = site.pages.map((p) => p.recipe);
  const others = (byCategory(siblings, [f.category])[0]?.recipes ?? []).filter((r) => r !== recipe);
  const more = others.length
    ? `<section class="more">
<h2 id="more-recipes">More in ${esc(f.category)}</h2>
<ul class="recipe-list">
${others.map((r) => recipeCard(r, '../')).join('\n')}
</ul>
</section>`
    : `<section class="more"><p><a href="../">All recipes</a></p></section>`;
  // Steps are Cypher and run here; a `partial` recipe also shows other code.
  const partial = f.browser === 'partial' ? ' Code in other languages is shown, not run.' : '';
  const hash = playgroundHash(recipe.slug);
  // The graph view colours each label as the recipe's data model does.
  const labels = `\n<script type="application/json" id="label-colors">${JSON.stringify(page.labels).replaceAll('<', '\\u003c')}</script>`;
  const body = `<div class="container recipe-container">
<article class="recipe-page">
<header class="recipe-head">
  <p class="eyebrow"><a href="../">Cookbook</a> / ${esc(f.category)} · ${esc(f.level)}</p>
  <h1>${esc(f.title)}</h1>
  <p class="lede">${env.lede ? site.md.renderInline(env.lede, env) : esc(f.summary)}</p>
  ${licenseNote}
</header>
<div class="engine-bar" id="engineBar" hidden>
  <span class="engine-status"><span class="dot"></span><span class="engine-text">Each step runs in your browser. Press Run to load the engine (${mb(site.wasmGzip)}); earlier steps run first.${partial}</span></span>
  <span class="engine-actions">
    <button class="btn" type="button" data-action="run-all">Run all</button>
    <button class="btn" type="button" data-action="reset">Reset</button>
    <button class="btn danger" type="button" data-action="stop" hidden>Stop</button>
    <a class="btn" href="../../playground/${esc(hash)}" target="_blank" rel="noopener" title="Run every step in the playground, then query the graph">Open in playground${EXT_ICON}</a>
  </span>
</div>
${tocNav}
<noscript><p class="noscript-note">JavaScript is off, so the steps below show their recorded outputs only.</p></noscript>
<div class="recipe-body">
${html}
</div>
${about}
${more}
</article>
</div>`;
  return shell(site, {
    title: `${f.title} · Relate cookbook`,
    description: f.summary,
    canonical: `${site.cookbookUrl}${recipe.slug}/`,
    depth: 2,
    body,
    head: labels,
    footerLinks: '<a href="index.md">Markdown</a>',
    // On a recipe page the nav's Playground link opens this recipe too.
    playground: esc(hash),
  });
}

/** The cookbook's index page. */
export function indexPage(site) {
  const recipes = site.pages.map((p) => p.recipe);
  const sections = byCategory(recipes, site.categories)
    .map(
      (g) => `<section class="category" id="${slugify(g.category, new Set())}">
<h2>${esc(g.category)}</h2>
<ul class="recipe-list">
${g.recipes.map((r) => recipeCard(r, '')).join('\n')}
</ul>
</section>`,
    )
    .join('\n');
  const readme = site.links.publicLink(
    `${site.repoUrl}/blob/${site.ref}/docs/cookbook/README.md#running-a-recipe-yourself`,
    'the cookbook README',
  );
  const source = site.links.publicLink(`${site.repoUrl}/tree/${site.ref}/docs/cookbook`, 'docs/cookbook');
  const foot =
    'The same Cypher runs from the CLI and from Python. Every recipe page publishes its data as JSON files ' +
    'beside it, and its source as markdown' +
    (readme ? `; ${readme} shows how to load a recipe's data yourself` : '') +
    (source ? `, and the recipes live in ${source}` : '') +
    '.';
  const body = `<div class="container">
<header class="page-head">
  <p class="eyebrow"><a href="../">relate</a> / cookbook</p>
  <h1>Cookbook</h1>
  <p class="lede">Recipes that solve one problem each, with a small dataset and plain Cypher. Every step runs in your browser on Relate compiled to WebAssembly, and every recorded result is checked against the engine in CI.</p>
</header>
${sections}
<p class="index-foot">${foot}</p>
</div>`;
  return shell(site, {
    title: 'Cookbook · Relate',
    description:
      'Runnable Relate recipes: entity resolution, geospatial tools, dependency analysis, hybrid search, agent memory, and more, each running in your browser.',
    canonical: site.cookbookUrl,
    depth: 1,
    body,
  });
}

/** The cookbook's llms.txt. */
export function llmsTxt(site) {
  const lines = [
    '# Relate cookbook',
    '',
    '> Task-shaped recipes for Relate, the embedded openCypher graph database. Each recipe is one markdown file: runnable `cypher cell=<id>` steps, `json data=<name>` blocks holding their parameters, and after each step its recorded result as a `table` block or, for nodes and paths, a `gram` block. CI checks every result against the engine.',
    '',
    "A step with `params=<name>` runs with that data block's JSON object as its parameters (CLI: `--param name=<json>`; Python: `db.execute(query, params)`). Each data block is also published beside its page as `data/<name>.json`.",
    '',
  ];
  for (const g of byCategory(site.pages.map((p) => p.recipe), site.categories)) {
    lines.push(`## ${g.category}`, '');
    for (const r of g.recipes) lines.push(`- [${r.front.title}](${site.cookbookUrl}${r.slug}/index.md): ${r.front.summary}`);
    lines.push('');
  }
  return lines.join('\n');
}

/**
 * The hand-off the playground reads (ADR-0134): format 1,
 * `{format, slug, title, cells: [{id, code, paramsPath, setup, expectError}]}`,
 * `paramsPath` relative to the recipe page.
 */
export function recipeJson(recipe) {
  return (
    JSON.stringify(
      {
        format: 1,
        slug: recipe.slug,
        title: recipe.front.title,
        cells: recipe.cells.map((c) => ({
          id: c.id,
          code: c.code,
          paramsPath: c.params ? dataPath(c.params) : null,
          setup: c.setup,
          expectError: c.expectError,
        })),
      },
      null,
      2,
    ) + '\n'
  );
}
