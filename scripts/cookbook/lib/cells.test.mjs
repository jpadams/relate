import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { surprise } from './cells.mjs';

// The cases the native harness's `run::surprise` is tested with too.
const CASES = JSON.parse(readFileSync(new URL('./cells.cases.json', import.meta.url), 'utf8'));

test('surprise reports an unexpected error or a missing one', () => {
  for (const { expectError, last, problem } of CASES) {
    assert.equal(surprise({ id: 'a', expectError }, last), problem, JSON.stringify({ expectError, last }));
  }
});
