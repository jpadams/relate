// The text of the data-model drawings (ADR-0135): fonts, a node's label and
// properties wrapped to fit a small circle, a relationship's caption. The
// caller measures text: `measure(text, font)` gives its width in pixels for
// a font of FONTS.

/** The drawing's fonts; `fontSpec` names one the way canvas measures it. */
export const FONTS = {
  label: { weight: 600, size: 14 },
  props: { weight: 400, size: 12 },
  type: { weight: 600, size: 12 },
  relProps: { weight: 400, size: 11 },
};
export const fontSpec = (f) => `${f.weight} ${f.size}px "Public Sans"`;

export const LINE = 1.3; // line height, in font sizes
const MIN_R = 34; // smallest node radius
const NODE_PAD = 9; // between a node's text and its circle

// The ways to break `words` (with measured `widths`) into lines joined by
// ` · ` (`sep` wide), in order: every way for a short list, greedy fills of
// a range of widths for a long one.
const EVERY_WAY = 10;
function wraps(words, widths, sep) {
  const line = (from, to) => ({
    text: words.slice(from, to).join(' · '),
    w: widths.slice(from, to).reduce((a, b) => a + b, 0) + sep * (to - from - 1),
  });
  if (words.length === 0) return [[]];
  if (words.length <= EVERY_WAY) {
    const out = [];
    for (let mask = 0; mask < 2 ** (words.length - 1); mask++) {
      const lines = [];
      let from = 0;
      for (let i = 1; i <= words.length; i++) {
        if (i === words.length || mask & (1 << (i - 1))) {
          lines.push(line(from, i));
          from = i;
        }
      }
      out.push(lines);
    }
    return out;
  }
  const out = [];
  for (let max = 40; max <= 360; max += 8) {
    const lines = [];
    let from = 0;
    for (let i = 1; i <= words.length; i++) {
      if (i === words.length || line(from, i + 1).w > max) {
        lines.push(line(from, i));
        from = i;
      }
    }
    out.push(lines);
  }
  return out;
}

// Lines stacked around y = 0; each gets `y`, the centre of its line.
function stack(lines) {
  const heights = lines.map((l) => FONTS[l.font].size * LINE);
  let y = -heights.reduce((a, b) => a + b, 0) / 2;
  return lines.map((l, i) => {
    const line = { ...l, y: y + heights[i] / 2 };
    y += heights[i];
    return line;
  });
}

// A label on one line, or split once where a word or label starts
// (`IndustryCategory`, `Doc:ADR`).
function labelLines(label) {
  const cuts = [...label.matchAll(/(?<=[a-z])(?=[A-Z])|(?<=:)(?=.)/g)].map((m) => m.index);
  return [[label], ...cuts.map((c) => [label.slice(0, c), label.slice(c)])];
}

// The smallest circle can hold a tall column of one property per line, which
// reads badly; a block taller than it is wide costs TALL per pixel of the
// difference, and a split label must save more than SPLIT_GAIN.
const SPLIT_GAIN = 8;
const TALL = 0.5;

/**
 * A node's text ({label, props}), wrapped to fit a small circle: {r, lines},
 * the circle's radius and the lines {text, font, w, y} centred on it.
 */
export function nodeText(node, measure) {
  const sep = measure(' · ', FONTS.props);
  const widths = node.props.map((p) => measure(p, FONTS.props));
  let best = null;
  const propLines = wraps(node.props, widths, sep);
  for (const parts of labelLines(node.label)) {
    const label = parts.map((text) => ({ text, font: 'label', w: measure(text, FONTS.label) }));
    const penalty = parts.length > 1 ? SPLIT_GAIN : 0;
    for (const props of propLines) {
      const lines = stack([...label, ...props.map((l) => ({ ...l, font: 'props' }))]);
      const r = Math.max(...lines.map((l) => Math.hypot(l.w / 2, Math.abs(l.y) + (FONTS[l.font].size * LINE) / 2)));
      const height = lines.reduce((h, l) => h + FONTS[l.font].size * LINE, 0);
      const score = r + penalty + TALL * Math.max(0, height - Math.max(...lines.map((l) => l.w)));
      if (!best || score < best.score - 0.5) best = { r, score, lines };
    }
  }
  return { r: Math.max(MIN_R, best.r + NODE_PAD), lines: best.lines };
}

/**
 * The size of a relationship's caption ({type, props}): its type on the
 * line, its properties below. {w, typeH, propsH, h}.
 */
export function captionSize(edge, measure) {
  const typeH = FONTS.type.size * LINE;
  const propsH = edge.props ? FONTS.relProps.size * LINE : 0;
  const w = Math.max(measure(edge.type, FONTS.type), edge.props ? measure(edge.props, FONTS.relProps) : 0);
  return { w, typeH, propsH, h: typeH + propsH };
}
