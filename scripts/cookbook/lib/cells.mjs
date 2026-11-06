// The cell policy every runner shares (ADR-0130): cells run in order, and a
// run stops at the first cell that goes other than its recipe says. The page
// runtime, the site build, and the playground use this; the native harness
// has the same rule (`run::surprise` in testing/relate-cookbook), and
// both are tested with cells.cases.json. The JSDoc types are the playground's
// TypeScript types for this module.

/** @typedef {{error?: string | null} | null} LastEnvelope a cell's last wire envelope (parsed), or null when nothing ran */

/**
 * Whether a cell failed. A script stops at its first error, so that is its
 * last envelope's.
 * @param {LastEnvelope} last
 * @returns {boolean}
 */
export const failed = (last) => (last?.error ?? null) !== null;

/**
 * How a cell's run differs from what its recipe says, or null: an error
 * where none is expected, or success where the cell is `expectError`.
 * @param {{id: string, expectError: boolean}} cell
 * @param {LastEnvelope} last
 * @returns {string | null}
 */
export function surprise(cell, last) {
  if (!cell.expectError && failed(last)) return `cell \`${cell.id}\` failed: ${last.error}`;
  if (cell.expectError && !failed(last)) return `cell \`${cell.id}\` is marked expect=error but succeeded`;
  return null;
}
