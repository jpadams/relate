// Node label colours, shared by the graph view (runtime/graph.js) and the
// recipe diagrams (ADR-0135): the palette Neo4j's graph tools (Bloom, NVL)
// draw labels in, in their order. Every colour reads with dark text in
// either theme.

import { TOKENS } from './tokens.mjs';

/** The palette by name, in the tools' order. */
export const PALETTE_BY_NAME = {
  yellow: '#FFDF81',
  lilac: '#C990C0',
  orange: '#F79767',
  cyan: '#56C7E4',
  red: '#F16767',
  beige: '#D8C7AE',
  green: '#8DCC92',
  pink: '#ECB4C9',
  blue: '#4D8DDA',
  amber: '#FFC354',
  rose: '#DA7294',
  teal: '#579380',
};

/** The palette in order: a diagram's class `lN` is entry N. */
export const PALETTE = Object.values(PALETTE_BY_NAME);

/** A node's colour when it has no label colour. */
export const NEUTRAL = '#A5ABB6';

/** Text on a label colour, in either theme: the light theme's text. */
export const LABEL_TEXT = TOKENS.light.fg;

/** The outline of a node filled with `fill` (#RRGGBB): the same hue, darker. */
export function outline(fill) {
  const dark = [1, 3, 5].map((i) => Math.round(parseInt(fill.slice(i, i + 2), 16) * 0.78));
  return `#${dark.map((c) => c.toString(16).padStart(2, '0')).join('').toUpperCase()}`;
}
