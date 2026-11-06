// The site's colour tokens, per theme: the one source for the page CSS
// (build.mjs writes the `:root` blocks of runtime/cookbook.css from these)
// and for the diagrams (diagrams/render.mjs, schema-graph.mjs). They mirror
// the homepage (gh-pages index.html, neo4j-cli design system).

const LIGHT = {
  bg: '#ffffff',
  bgAlt: '#F7F8FA',
  fg: '#0A1014',
  fgMuted: '#4A5664',
  fgFaint: '#6B7A88',
  border: '#E3E8EE',
  borderStrong: '#C9D2DB',
  accent: '#006FD6',
  accentHover: '#0061BD',
  success: '#158838',
  danger: '#d43300',
  codeBg: '#0F141A',
  outBg: '#F7F8FA',
  shadowRaised: '0 1px 2px rgba(10, 16, 20, 0.06), 0 1px 3px rgba(10, 16, 20, 0.04)',
  shadowOverlay: '0 4px 16px rgba(10, 16, 20, 0.10)',
};

const DARK = {
  bg: '#0B0E12',
  bgAlt: '#11161C',
  fg: '#ECEFF3',
  fgMuted: '#9AA8B4',
  fgFaint: '#6B7A88',
  border: '#1E2530',
  borderStrong: '#2A3340',
  accent: '#4FA3F0',
  accentHover: '#6FB7FA',
  success: '#3FBF6A',
  danger: '#F96746',
  codeBg: '#161B22',
  outBg: '#11161C',
  shadowRaised: '0 1px 2px rgba(0, 0, 0, 0.5), 0 1px 3px rgba(0, 0, 0, 0.3)',
  shadowOverlay: '0 4px 16px rgba(0, 0, 0, 0.5)',
};

// Code blocks are dark in both themes, so they are bordered as dark is.
const CODE = { codeFg: '#E3E8EE', codeBorder: DARK.border };

export const TOKENS = {
  light: { ...LIGHT, ...CODE },
  dark: { ...DARK, ...CODE },
};

const cssName = (key) => `--${key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
const block = (theme, indent) =>
  Object.entries(TOKENS[theme])
    .map(([key, value]) => `${indent}${cssName(key)}: ${value};`)
    .join('\n');

/**
 * The CSS that sets the tokens: light on `:root`, dark for
 * `[data-theme="dark"]` and, unless the reader chose light, for a dark
 * system preference.
 */
export function tokensCss() {
  return [
    `:root {\n${block('light', '  ')}\n  color-scheme: light;\n}`,
    `[data-theme="dark"] {\n${block('dark', '  ')}\n  color-scheme: dark;\n}`,
    `@media (prefers-color-scheme: dark) {\n  :root:not([data-theme="light"]) {\n${block('dark', '    ')}\n    color-scheme: dark;\n  }\n}`,
  ].join('\n\n');
}
