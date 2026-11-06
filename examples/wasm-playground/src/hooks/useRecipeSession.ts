// Cookbook → playground hand-off (ADR-0130, ADR-0134). When the page is
// opened from a cookbook step (#recipe=<slug>&cell=<id>) or a whole recipe
// (#recipe=<slug>), fetch the recipe and work out what loading it does. When
// the reader confirms, fetch every input first, then reset the database and
// replay the steps, stopping at the first that goes other than the recipe
// says (the cookbook's own cell policy, scripts/cookbook/lib/cells.mjs). A
// network failure changes nothing.

import { useCallback, useEffect, useState } from 'react';

import { surprise } from '../../../../scripts/cookbook/lib/cells.mjs';
import { engine, EnginePoisonedError } from '../lib/engine';
import {
  fetchParams,
  fetchRecipe,
  parseRecipeLink,
  resolveRecipeLink,
  type Recipe,
  type RecipePlan,
} from '../lib/recipe-link';

/** A recipe step's parameters, passed verbatim with every query. */
export interface RecipeParams {
  text: string;
  label: string;
}

/** Where the linked recipe stands before it is loaded. */
export type RecipeState =
  | { kind: 'loading' }
  | { kind: 'failed'; message: string }
  | { kind: 'missing-step'; recipe: Recipe }
  | RecipePlan;

/** What a load did: the steps it ran, and the linked step for the editor. */
export interface LoadedRecipe {
  title: string;
  replayed: number;
  /** The linked step's Cypher and parameters; null when the whole recipe ran. */
  step: { code: string; params: RecipeParams | null } | null;
}

interface Options {
  /** Called right after the engine resets the database for the replay. */
  afterReset: () => void;
  onPoisoned: (e: EnginePoisonedError) => void;
}

export function useRecipeSession({ afterReset, onPoisoned }: Options) {
  const [link] = useState(() => parseRecipeLink(window.location.hash));
  const [state, setState] = useState<RecipeState>({ kind: 'loading' });
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    if (!link) return;
    fetchRecipe(link.slug).then(
      (recipe) => setState(resolveRecipeLink(recipe, link) ?? { kind: 'missing-step', recipe }),
      (e) => setState({ kind: 'failed', message: `Could not load the recipe: ${(e as Error).message}` }),
    );
  }, [link]);

  /** Loads the recipe; null if nothing loaded (see `error`). */
  const load = useCallback(async (): Promise<LoadedRecipe | null> => {
    if (state.kind !== 'whole' && state.kind !== 'step') return null;
    const { recipe, replay } = state;
    const target = state.kind === 'step' ? state.target : null;
    setIsLoading(true);
    setError(null);
    try {
      // Every parameter file first, each once, before the database changes.
      const paths = [...new Set([...replay, ...(target ? [target] : [])].flatMap((c) => c.paramsPath ?? []))];
      const texts = new Map(await Promise.all(paths.map(async (p) => [p, await fetchParams(recipe.slug, p)] as const)));
      const paramsAt = (path: string): string => {
        const text = texts.get(path);
        if (text === undefined) throw new Error(`${path} was not fetched`);
        return text;
      };

      await engine.reset();
      afterReset();
      for (const cell of replay) {
        const run = await engine.runScript(cell.code, cell.paramsPath === null ? null : paramsAt(cell.paramsPath));
        const problem = surprise(cell, run === null ? null : run.last);
        if (problem) {
          setError(`The recipe stopped: ${problem}`);
          return null;
        }
      }
      const step = target && {
        code: target.code,
        params:
          target.paramsPath === null
            ? null
            : { text: paramsAt(target.paramsPath), label: `${recipe.title} · ${target.paramsPath}` },
      };
      return { title: recipe.title, replayed: replay.length, step };
    } catch (e) {
      if (e instanceof EnginePoisonedError) onPoisoned(e);
      else setError((e as Error).message);
      return null;
    } finally {
      setIsLoading(false);
    }
  }, [state, afterReset, onPoisoned]);

  return { link, state, error, isLoading, load };
}
