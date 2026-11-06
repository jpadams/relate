import assert from 'node:assert/strict';
import { test } from 'node:test';

import { TOKENS, tokensCss } from './tokens.mjs';

test('both themes set the same tokens', () => {
  assert.deepEqual(Object.keys(TOKENS.dark), Object.keys(TOKENS.light));
});

test('tokensCss writes light, the dark theme, and the dark system preference', () => {
  const css = tokensCss();
  assert.match(css, /^:root \{\n  --bg: #ffffff;\n  --bg-alt: #F7F8FA;/);
  assert.match(css, /\[data-theme="dark"\] \{\n  --bg: #0B0E12;/);
  assert.match(css, /@media \(prefers-color-scheme: dark\) \{\n  :root:not\(\[data-theme="light"\]\) \{\n    --bg: #0B0E12;/);
  assert.equal(css.match(/--shadow-overlay/g).length, 3);
});
