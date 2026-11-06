import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { checkImports, checkLinks, excludePatterns, linkPolicy, rewriteHref } from './links.mjs';

// The repository's list, as the build reads it by default.
const LINKS = linkPolicy(readFileSync(new URL('../../../lychee.toml', import.meta.url), 'utf8'));

const SITE = 'https://relatedb.ai/';
const page = (links) => links.map((h) => `<a href="${h}">x</a>`).join('\n');

function check(links, siteUrl = SITE, exempt = undefined) {
  const pages = new Map([
    ['index.html', '<h2 id="agent-memory">Agent memory</h2>'],
    ['a/index.html', `<h2 id="step">Step</h2>\n${page(links)}`],
  ]);
  const files = new Set([...pages.keys(), 'a/index.md', 'a/data/places.json', 'assets/cookbook.css']);
  return checkLinks(pages, files, { siteUrl, links: LINKS, exempt });
}

test('links inside the cookbook must exist, fragments included', () => {
  assert.deepEqual(
    check(['../', '../#agent-memory', '#step', 'index.md', 'data/places.json', `${SITE}cookbook/a/`]),
    [],
  );
  assert.deepEqual(check(['data/nope.json', '../b/', '#missing', '../#nope']), [
    'a/index.html: data/nope.json does not exist',
    'a/index.html: ../b/ does not exist',
    'a/index.html: #missing has no #missing',
    'a/index.html: ../#nope has no #nope',
  ]);
});

test('links out of the cookbook go to known site pages', () => {
  assert.deepEqual(check(['../../', '../../#quickstart', '../../playground/#recipe=a&amp;cell=b']), []);
  assert.deepEqual(check(['../../install.sh']), [
    'a/index.html: ../../install.sh leaves the cookbook for an unknown page',
  ]);
});

test('the site path comes from the site URL', () => {
  const site = 'https://example.org/docs/';
  assert.deepEqual(check(['../', '../../playground/', `${site}cookbook/a/index.md`], site), []);
  assert.deepEqual(check(['../../../elsewhere/'], site), ['a/index.html: ../../../elsewhere/ leaves the site']);
});

test('not-yet-public URLs fail; other external links pass', () => {
  const links = [
    'https://github.com/relatedb/relate/blob/main/docs/adr/0035.md',
    'https://relatedb.ai/cookbook/agent-memory',
    'https://deps.dev/',
    'mailto:someone@example.com',
  ];
  const problems = check(links);
  assert.equal(problems.length, 2);
  assert.match(problems[0], /is not public yet$/);
});

test('an exempt URL passes while not public, matched exactly (ADR-0288)', () => {
  const repo = 'https://github.com/relatedb/relate';
  assert.deepEqual(check([repo]), [`a/index.html: ${repo} is not public yet`]);
  assert.deepEqual(check([repo], SITE, new Set([repo])), []);
  assert.deepEqual(check([`${repo}/blob/main/README.md`, `${repo}/`], SITE, new Set([repo])), [
    `a/index.html: ${repo}/blob/main/README.md is not public yet`,
    `a/index.html: ${repo}/ is not public yet`,
  ]);
});

test('the not-yet-public list is lychee.toml\'s exclude list', () => {
  const toml = "exclude = [\n  '^https://a\\.example/x',\n  # a comment\n  \"^https://b\\\\.example\", # why\n]\nother = ['^nope']\n";
  assert.deepEqual(excludePatterns(toml).map(String), ['/^https:\\/\\/a\\.example\\/x/', '/^https:\\/\\/b\\.example/']);
  assert.equal(LINKS.isNotYetPublic('https://github.com/relatedb/relate/blob/main/README.md'), true);
  assert.equal(LINKS.isNotYetPublic('https://relatedb.ai'), true);
  assert.equal(LINKS.isNotYetPublic('https://github.com/gram-data/tree-sitter-gram'), false);
  assert.equal(linkPolicy('exclude = []').isNotYetPublic('https://github.com/relatedb/relate'), false);
});

test('a list the build cannot read fails instead of publishing everything', () => {
  assert.throws(() => excludePatterns('timeout = 20'), /`exclude` must be a top-level array of strings/);
  assert.throws(() => excludePatterns("[links]\nexclude = ['a']"), /`exclude` must be/);
  assert.throws(() => excludePatterns('exclude = [1]'), /`exclude` must be/);
  assert.throws(() => excludePatterns("exclude = [\n  'a'\n  'b'\n]"), /Invalid TOML document/);
  assert.throws(() => excludePatterns('exclude = ["a\\.b"]'), /Invalid TOML document/);
});

test('rewriteHref maps GitHub-relative links onto the site', () => {
  const opts = { slugs: new Set(['nearby-places']), repoUrl: 'https://git.example/r', ref: 'v1', links: LINKS };
  assert.equal(rewriteHref('README.md#running', opts), '../#running');
  assert.equal(rewriteHref('nearby-places.md', opts), '../nearby-places/');
  assert.equal(rewriteHref('#x', opts), '#x');
  assert.equal(rewriteHref('../adr/0135-cookbook-diagrams.md', opts), 'https://git.example/r/blob/v1/docs/adr/0135-cookbook-diagrams.md');
  assert.equal(rewriteHref('https://deps.dev/', opts), 'https://deps.dev/');
  assert.equal(rewriteHref('https://github.com/relatedb/relate', opts), null);
  const hosted = { ...opts, repoUrl: 'https://github.com/relatedb/relate' };
  assert.equal(rewriteHref('../adr/0135-cookbook-diagrams.md', hosted), null);
});

test('script imports must be files the build writes, bytes or text (the wasm glue\'s snippets)', () => {
  const bytes = (text) => new TextEncoder().encode(text);
  const files = new Map([
    ['assets/worker.js', bytes('import i,{RelateDb as p}from"../wasm/relate.js";import{isWasmTrap as l}from"../wasm/trap.js";')],
    ['assets/cookbook.js', 'import{a as b}from"./chunk-A.js";const g=()=>import("./graph.js");import"./side.js";'],
    ['assets/chunk-A.js', 'export const a = 1;'],
    ['wasm/relate.js', bytes("import { prepareOpfs } from './snippets/relate-wasm-0123/js/opfs.js';\n")],
    ['wasm/trap.js', 'export const isWasmTrap = () => false;'],
  ]);
  assert.deepEqual(checkImports(files), [
    'assets/cookbook.js: ./graph.js does not exist',
    'assets/cookbook.js: ./side.js does not exist',
    'wasm/relate.js: ./snippets/relate-wasm-0123/js/opfs.js does not exist',
  ]);
  files.set('assets/graph.js', 'export {};');
  files.set('assets/side.js', '');
  files.set('wasm/snippets/relate-wasm-0123/js/opfs.js', bytes('export function prepareOpfs() {}'));
  assert.deepEqual(checkImports(files), []);
});

test('only relative imports of scripts are checked', () => {
  const files = new Map([
    ['a.js', "import 'node:fs';import x from 'pkg';Array.from('./n.js');y.import('./n.js');new URL('./n.js',import.meta.url);"],
    ['index.html', '<script type="module" src="./n.js"></script><p>import "./n.js"</p>'],
    ['wasm/relate_bg.wasm', new Uint8Array([0, 97, 115, 109])],
  ]);
  assert.deepEqual(checkImports(files), []);
});
