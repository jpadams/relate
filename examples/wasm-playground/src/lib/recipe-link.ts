// Cookbook → playground hand-off (ADR-0130, ADR-0134).
//
// A cookbook cell links to the playground, `playground/#recipe=<slug>&cell=<id>`
// beside `cookbook/` under the site root. The playground fetches
// `../cookbook/<slug>/recipe.json`, relative to itself (written by
// scripts/cookbook/build.mjs, format 1), asks the reader to confirm, replays
// the steps before the target cell, and puts the target cell in the editor
// unrun. A recipe page's "Open in playground" links to `#recipe=<slug>` with no
// cell: the playground runs every step and leaves the editor empty.

/** The recipe.json format this reads. */
export const RECIPE_FORMAT = 1;

export interface RecipeCell {
  id: string;
  code: string;
  /** Path of the step's JSON parameter file, relative to the recipe page. */
  paramsPath: string | null;
  setup: boolean;
  expectError: boolean;
}

export interface Recipe {
  slug: string;
  title: string;
  cells: RecipeCell[];
}

export interface RecipeLink {
  slug: string;
  /** The step to open; null opens the whole recipe. */
  cell: string | null;
}

/**
 * What loading a link does: replay `replay` in order, then, for a step link,
 * put `target` in the editor.
 */
export type RecipePlan =
  | { kind: 'whole'; recipe: Recipe; replay: RecipeCell[] }
  | { kind: 'step'; recipe: Recipe; replay: RecipeCell[]; target: RecipeCell };

const ID = /^[a-z0-9-]+$/;

/** `#recipe=<slug>[&cell=<id>]` → the link, or null for any other hash. */
export function parseRecipeLink(hash: string): RecipeLink | null {
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  const slug = params.get('recipe');
  const cell = params.get('cell');
  if (!slug || !ID.test(slug)) return null;
  if (cell !== null && !ID.test(cell)) return null;
  return { slug, cell };
}

/** The plan for `link`, or null when the recipe has no step with its id. */
export function resolveRecipeLink(recipe: Recipe, link: RecipeLink): RecipePlan | null {
  if (link.cell === null) return { kind: 'whole', recipe, replay: recipe.cells };
  const index = recipe.cells.findIndex((c) => c.id === link.cell);
  if (index < 0) return null;
  return { kind: 'step', recipe, replay: recipe.cells.slice(0, index), target: recipe.cells[index] };
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

function isRecipeCell(v: unknown): v is RecipeCell {
  return (
    isObject(v) &&
    typeof v.id === 'string' &&
    typeof v.code === 'string' &&
    (v.paramsPath === null || typeof v.paramsPath === 'string') &&
    typeof v.setup === 'boolean' &&
    typeof v.expectError === 'boolean'
  );
}

/** A fetched recipe.json as a Recipe; throws when it is another format or shape. */
export function toRecipe(json: unknown): Recipe {
  if (!isObject(json) || json.format !== RECIPE_FORMAT) {
    const format = isObject(json) ? json.format : undefined;
    throw new Error(`recipe.json has format ${String(format)}; this playground reads format ${RECIPE_FORMAT}`);
  }
  const { slug, title, cells } = json;
  if (typeof slug !== 'string' || typeof title !== 'string' || !Array.isArray(cells) || !cells.every(isRecipeCell)) {
    throw new Error('recipe.json is not a recipe (format 1)');
  }
  return { slug, title, cells };
}

/** The cookbook's root URL: a sibling of the playground on the site. */
function cookbookBase(): URL {
  return new URL('../cookbook/', document.baseURI);
}

export function recipePageUrl(link: RecipeLink): string {
  const hash = link.cell ? `#cell-${link.cell}` : '';
  return new URL(`${link.slug}/${hash}`, cookbookBase()).href;
}

async function fetchOk(url: URL): Promise<Response> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url.pathname}: HTTP ${res.status}`);
  return res;
}

export async function fetchRecipe(slug: string): Promise<Recipe> {
  const res = await fetchOk(new URL(`${slug}/recipe.json`, cookbookBase()));
  return toRecipe(await res.json());
}

/** A step's parameter file, as text: passed to the engine verbatim. */
export async function fetchParams(slug: string, path: string): Promise<string> {
  const res = await fetchOk(new URL(`${slug}/${path}`, cookbookBase()));
  return res.text();
}
