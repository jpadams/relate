#!/usr/bin/env node
// Runs the documented WASM API examples (docs/api/wasm/examples/*.js)
// against a built bundle (AGENTS.md §2: examples are tests). Each example
// runs as a module next to a copy of the bundle, as the docs describe.
//
// Usage: node surfaces/relate-wasm/tests/examples.mjs [PKG_DIR]
//   PKG_DIR  the bundle scripts/build-wasm.sh wrote (default: surfaces/relate-wasm/pkg)

import { copyFileSync, cpSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const pkg = resolve(process.argv[2] ?? join(repo, 'surfaces/relate-wasm/pkg'));
const examples = join(repo, 'docs/api/wasm/examples');

// `init()` fetches relate_bg.wasm by URL; Node's fetch has no file:
// scheme, so serve file URLs from disk.
const realFetch = globalThis.fetch;
globalThis.fetch = async (input, init) => {
  const url = input instanceof URL ? input : new URL(String(input), 'file:///');
  if (url.protocol !== 'file:') return realFetch(input, init);
  const type = url.pathname.endsWith('.wasm') ? 'application/wasm' : 'application/octet-stream';
  return new Response(readFileSync(fileURLToPath(url)), { headers: { 'content-type': type } });
};

const scratch = mkdtempSync(join(tmpdir(), 'relate-wasm-examples-'));
const failures = [];
let ran = 0;
try {
  // pkg/snippets is the wasm-bindgen snippet directory (opfs.js). copyFile
  // rejects a directory with EISDIR.
  for (const f of readdirSync(pkg)) {
    const from = join(pkg, f);
    const to = join(scratch, f);
    if (statSync(from).isDirectory()) cpSync(from, to, { recursive: true });
    else copyFileSync(from, to);
  }
  for (const name of readdirSync(examples).filter((f) => f.endsWith('.js')).sort()) {
    copyFileSync(join(examples, name), join(scratch, name));
    try {
      await import(pathToFileURL(join(scratch, name)).href);
      ran++;
    } catch (e) {
      failures.push(`docs/api/wasm/examples/${name}: ${e.stack ?? e}`);
    }
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
if (failures.length) {
  console.error(failures.join('\n\n'));
  process.exit(1);
}
console.log(`ok: ${ran} WASM API examples ran against ${pkg}`);
