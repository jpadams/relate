// How a cookbook page shows a result (ADR-0130). The recording itself (kind,
// text, table cells, graph) comes from the WASM `renderResult`, the renderer
// the native harness uses; this module only lays it out. Shared by build.mjs
// (recorded results) and the page runtime (live ones).

import { esc } from './html.mjs';

/** The last statement's envelope of a run (wire JSON text per statement), or null. */
export const lastEnvelope = (envelopes) => (envelopes.length ? JSON.parse(envelopes.at(-1)) : null);

/**
 * Whether a recording's graph (`{nodes, rels}`, or null for a table) has
 * anything the graph view can draw: it draws nodes, and the relationships
 * between them.
 */
export const canDraw = (graph) => (graph?.nodes.length ?? 0) > 0;

/**
 * A table recording's cells (`{columns, rows, numeric, summary}`) as HTML:
 * the table in a scroll box, then the row count and counters. Cells are the
 * Cypher literals the `table` block records; numeric columns are
 * right-aligned. The build writes this into the page and the runtime writes
 * it for live results, so both look alike.
 */
export function tableHtml({ columns, rows, numeric, summary }) {
  const cls = (i) => (numeric[i] ? ' class="num"' : '');
  const head = columns.map((c, i) => `<th scope="col"${cls(i)}>${esc(c)}</th>`).join('');
  const body = rows.map((r) => `<tr>${r.map((v, i) => `<td${cls(i)}>${esc(v)}</td>`).join('')}</tr>`).join('');
  return (
    `<div class="table-scroll" tabindex="0"><table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></div>` +
    `<p class="out-summary">${esc(summary)}</p>`
  );
}
