// The recipes, as the Rust harness parses them (ADR-0132): `cookbook-export`
// prints the parse as JSON, so the Node tools have no parser of their own.
// It needs cargo, and builds without the engine in seconds.

import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');

/** The export format this reads. */
export const EXPORT_FORMAT = 1;

/**
 * The parsed cookbook under `root`: {categories, inlineDataBytes, recipes}.
 * Each recipe is {slug, file, front, heading, bodyLine, narrative, cells,
 * data, diagrams, fences}: `narrative` is the text after the front matter up
 * to the `## Data` section, which the page renders, and `data` maps a block
 * name to its JSON text. Throws, with
 * the harness's messages, when a recipe does not parse or (unless
 * `validate` is false) does not validate.
 */
export function loadCookbook(root, { validate = true } = {}) {
  let out;
  try {
    out = execFileSync(
      'cargo',
      ['run', '--quiet', '--package', 'relate-cookbook', '--no-default-features',
        '--bin', 'cookbook-export', '--', '--cookbook', resolve(root), ...(validate ? [] : ['--no-validate'])],
      { cwd: repo, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] },
    );
  } catch (e) {
    throw new Error(`cookbook-export failed:\n${e.stderr || e.message}`);
  }
  const cookbook = JSON.parse(out);
  if (cookbook.format !== EXPORT_FORMAT) {
    throw new Error(`cookbook-export wrote format ${cookbook.format}; this reads format ${EXPORT_FORMAT}`);
  }
  return {
    ...cookbook,
    recipes: cookbook.recipes.map((r) => ({ ...r, data: new Map(r.data.map((d) => [d.name, d.text])) })),
  };
}
