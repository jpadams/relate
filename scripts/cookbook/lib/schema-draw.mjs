// Drawing a laid-out data model (ADR-0135) as an SVG for the site, in its
// tokens (lib/tokens.mjs) for each theme.

import { esc } from './html.mjs';
import { LABEL_TEXT, NEUTRAL, outline } from './palette.mjs';
import { extent } from './schema-geometry.mjs';
import { FONTS } from './schema-text.mjs';
import { TOKENS } from './tokens.mjs';

// Colours per theme: relationship lines the neutral label colour in light,
// and the faint text colour in dark, where that grey would glare; captions
// the site's text colours. Node text is LABEL_TEXT on every palette colour.
const THEME = {
  light: { bg: TOKENS.light.bg, rel: NEUTRAL, type: TOKENS.light.fg, props: TOKENS.light.fgMuted },
  dark: { bg: TOKENS.dark.bg, rel: TOKENS.dark.fgFaint, type: TOKENS.dark.fg, props: TOKENS.dark.fgMuted },
};

const num = (v) => String(Math.round(v * 10) / 10);
const pt = (p) => `${num(p.x)},${num(p.y)}`;

/**
 * The SVG of a laid-out data model (`layoutSchema`) for `theme` (light or
 * dark): `id` must be unique on the page; `title` names the diagram.
 */
export function drawSchema(layout, { id, theme, title }) {
  const c = THEME[theme];
  const { nodes, edges } = layout;

  const rels = edges.map((e) => {
    const g = e.geo;
    const line = `<path class="rel${e.dashed ? ' dash' : ''}" d="M${pt(g.start)} C${pt(g.c1)} ${pt(g.c2)} ${pt(g.end)}"/>`;
    const head = g.head ? `<path class="head" d="M${g.head.map(pt).join(' L')} Z"/>` : '';
    return line + head;
  });

  const circles = nodes.map((n) => {
    const text = n.lines
      .map((l) => `<text class="${l.font}" y="${num(l.y)}" dy="0.35em">${esc(l.text)}</text>`)
      .join('');
    return `<g class="node" transform="translate(${pt(n)})"><circle r="${num(n.r)}" fill="${n.color}" stroke="${outline(n.color)}"/>${text}</g>`;
  });

  const captions = edges
    .filter((e) => e.type || e.props)
    .map((e) => {
      const { x, y, angle, top } = e.geo.caption;
      const type = `<text class="type" y="${num(top + e.cap.typeH / 2)}" dy="0.35em">${esc(e.type)}</text>`;
      const props = e.props
        ? `<text class="rprops" y="${num(top + e.cap.typeH + e.cap.propsH / 2)}" dy="0.35em">${esc(e.props)}</text>`
        : '';
      const turn = angle ? ` rotate(${num(angle)})` : '';
      return `<g transform="translate(${pt({ x, y })})${turn}">${type}${props}</g>`;
    });

  const box = extent(nodes, edges);
  const pad = 12;
  const minX = Math.floor(box.minX - pad);
  const minY = Math.floor(box.minY - pad);
  const width = Math.ceil(box.minX + box.width + pad - minX);
  const height = Math.ceil(box.minY + box.height + pad - minY);

  const s = `#${id}`;
  const style = [
    `${s}{font-family:"Public Sans",system-ui,sans-serif}`,
    `${s} text{text-anchor:middle}`,
    `${s} .rel{fill:none;stroke:${c.rel};stroke-width:2}`,
    `${s} .dash{stroke-dasharray:6 4}`,
    `${s} .head{fill:${c.rel}}`,
    `${s} .node circle{stroke-width:2}`,
    `${s} .label{font-size:${FONTS.label.size}px;font-weight:${FONTS.label.weight};fill:${LABEL_TEXT}}`,
    `${s} .props{font-size:${FONTS.props.size}px;font-weight:${FONTS.props.weight};fill:${LABEL_TEXT};fill-opacity:0.78}`,
    `${s} .type,${s} .rprops{stroke:${c.bg};stroke-width:5px;stroke-linejoin:round;paint-order:stroke}`,
    `${s} .type{font-size:${FONTS.type.size}px;font-weight:${FONTS.type.weight};fill:${c.type}}`,
    `${s} .rprops{font-size:${FONTS.relProps.size}px;font-weight:${FONTS.relProps.weight};fill:${c.props}}`,
  ].join('');
  const desc = edges.map((e) => `${e.s.label} ${e.type || 'to'} ${e.t.label}`).join('; ');

  return (
    `<svg id="${id}" width="100%" xmlns="http://www.w3.org/2000/svg" style="max-width: ${width}px;" ` +
    `viewBox="${minX} ${minY} ${width} ${height}" role="img" aria-labelledby="${id}-title ${id}-desc">` +
    `<title id="${id}-title">${esc(title)}</title><desc id="${id}-desc">${esc(desc)}</desc>` +
    `<style>${style}</style>` +
    `<g>${rels.join('')}</g><g>${circles.join('')}</g><g>${captions.join('')}</g></svg>`
  );
}
