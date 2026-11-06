import assert from 'node:assert/strict';
import { test } from 'node:test';

import { createMarkdown, italicText, slugify } from './markdown.mjs';

const md = createMarkdown({
  rewriteHref: (href) => (href.startsWith('private:') ? null : href.replace(/\.md$/, '/')),
  siteUrl: 'https://site.example/',
});

function render(body) {
  const fences = [];
  const env = { fence: (token, line) => (fences.push([line, token.info]), `<fence ${line}>`) };
  return { html: md.render(body, env), env, fences };
}

test('the title and the opening italic paragraph are left out, and the lede kept', () => {
  const { html, env } = render('\n# Title\n\n*A lede\nover two lines.*\n\nFirst paragraph.\n');
  assert.equal(env.lede, 'A lede over two lines.');
  assert.equal(html, '<p>First paragraph.</p>\n');
  assert.equal(render('# T\n\nPlain opening.\n').env.lede, undefined);
});

test('fences keep their line; <details> wrappers are dropped', () => {
  const narrative = '# T\n\n```cypher cell=a\nRETURN 1\n```\n\n<details>\n<summary>More</summary>\n\nHidden on GitHub.\n\n</details>\n';
  const { html, fences } = render(narrative);
  assert.deepEqual(fences, [[2, 'cypher cell=a']]);
  assert.doesNotMatch(html, /details|summary/);
  assert.match(html, /Hidden on GitHub/);
});

test('headings get ids and make the contents list; callouts get a class', () => {
  const { html, env } = render('# T\n\n## Load `places`\n\n## Load `places`\n\n> **Note** careful\n');
  assert.deepEqual(env.toc, [
    { id: 'load-places', label: 'Load <code>places</code>' },
    { id: 'load-places-2', label: 'Load <code>places</code>' },
  ]);
  assert.match(html, /<blockquote class="callout">/);
});

test('links are rewritten, external ones open in a new tab, private ones become text', () => {
  const { html } = render('[a](other.md) [b](https://x.example/) [c](https://site.example/p) [d](private:x)');
  assert.match(html, /<a href="other\/">a<\/a>/);
  assert.match(html, /<a href="https:\/\/x\.example\/" target="_blank" rel="noopener">b<\/a>/);
  assert.match(html, /<a href="https:\/\/site\.example\/p">c<\/a>/);
  assert.match(html, / d<\/p>/);
});

test('inline HTML renders as text; text in a wrapper block is kept', () => {
  assert.match(render('a <b>c</b>').html, /a &lt;b&gt;c&lt;\/b&gt;/);
  const { html } = render('<details>\nplain words\n</details>\n');
  assert.equal(html, '<p>plain words</p>\n');
});

test('helpers', () => {
  const used = new Set();
  assert.equal(slugify('Hybrid recall: vector + text', used), 'hybrid-recall-vector-text');
  assert.equal(slugify('', used), 'section');
  assert.equal(italicText('_Underscores work too._'), 'Underscores work too.');
  assert.equal(italicText('*half italic* and not.'), null);
});
