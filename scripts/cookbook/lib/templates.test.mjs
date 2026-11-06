import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { linkPolicy } from './links.mjs';
import { byCategory, dataPath, indexPage, playgroundHash, recipeJson, shortRef } from './templates.mjs';

const recipe = (slug, title, category) => ({ slug, front: { title, category } });

test('byCategory keeps category order, sorts titles by bytes, and drops empty groups', () => {
  const rs = [recipe('b', 'beta', 'X'), recipe('a', 'Alpha', 'X'), recipe('c', 'Zed', 'Y'), recipe('d', 'alpha', 'X')];
  assert.deepEqual(
    byCategory(rs, ['Y', 'Z', 'X']).map((g) => [g.category, g.recipes.map((r) => r.slug)]),
    [
      ['Y', ['c']],
      ['X', ['a', 'd', 'b']],
    ],
  );
});

test('recipeJson is the playground hand-off, format 1', () => {
  const r = {
    slug: 's',
    front: { title: 'T' },
    cells: [
      { id: 'load', code: 'RETURN 1', params: 'rows', setup: true, expectError: false },
      { id: 'q', code: 'RETURN 2', params: null, setup: false, expectError: true },
    ],
  };
  assert.deepEqual(JSON.parse(recipeJson(r)), {
    format: 1,
    slug: 's',
    title: 'T',
    cells: [
      { id: 'load', code: 'RETURN 1', paramsPath: 'data/rows.json', setup: true, expectError: false },
      { id: 'q', code: 'RETURN 2', paramsPath: null, setup: false, expectError: true },
    ],
  });
});

test('the nav links the repository while it is not public; the footer does not (ADR-0288)', () => {
  const repoUrl = 'https://github.com/relatedb/relate';
  const site = (lychee) => ({
    cookbookUrl: 'https://relatedb.ai/cookbook/',
    repoUrl,
    ref: 'v1.0.0',
    categories: [],
    pages: [],
    links: linkPolicy(lychee),
  });
  const parts = (html) => ({
    nav: html.match(/<nav class="top-nav"[^>]*>([\s\S]*?)<\/nav>/)[1],
    footer: html.match(/<footer>([\s\S]*?)<\/footer>/)[1],
  });
  const github = `<a href="${repoUrl}" target="_blank" rel="noopener">GitHub</a>`;

  // The repository's own list, which still excludes the repository.
  const internal = parts(indexPage(site(readFileSync(new URL('../../../lychee.toml', import.meta.url), 'utf8'))));
  assert.deepEqual(
    [...internal.nav.matchAll(/<a [^>]*>(\w+)/g)].map((m) => m[1]),
    ['Examples', 'Reference', 'Quickstart', 'Cookbook', 'GitHub', 'Playground'],
  );
  assert.ok(internal.nav.includes(github));
  assert.doesNotMatch(internal.footer, /github\.com/);

  // Once public, the footer links it too; the nav is unchanged.
  const published = parts(indexPage(site('exclude = []')));
  assert.equal(published.nav, internal.nav);
  assert.ok(published.footer.includes(github));
});

test('small helpers', () => {
  assert.equal(dataPath('places'), 'data/places.json');
  assert.equal(playgroundHash('a b'), '#recipe=a%20b');
  assert.equal(playgroundHash('s', 'c-1'), '#recipe=s&cell=c-1');
  assert.equal(shortRef('24a64aed0d9e82f808aa0a3af3abfd6424bb6e8d'), '24a64ae');
  assert.equal(shortRef('v0.8.0'), 'v0.8.0');
});
