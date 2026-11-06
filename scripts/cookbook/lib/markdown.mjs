// Recipe markdown for the website (ADR-0130): markdown-it with the
// cookbook's rules. It renders a recipe's narrative (the text the recipe
// parser puts before `## Data`) exactly as the parser read it, so every fence
// keeps its line and the page builder looks it up in the parser's fence list
// instead of classifying it again.

import MarkdownIt from 'markdown-it';

/** `text` as a heading id, unique within `used`. */
export function slugify(text, used) {
  const base =
    text
      .toLowerCase()
      .replace(/<[^>]+>/g, '')
      .replace(/[^\p{L}\p{N}\s-]/gu, '')
      .trim()
      .replace(/\s+/g, '-') || 'section';
  let slug = base;
  for (let i = 2; used.has(slug); i++) slug = `${base}-${i}`;
  used.add(slug);
  return slug;
}

/** The inside of a paragraph that is wholly italic (`*…*` or `_…_`), or null. */
export function italicText(paragraph) {
  const m = /^([*_])(?![*_\s])([\s\S]*?[^\s*_\\])\1$/.exec(paragraph.trim());
  return m ? m[2].replace(/\s*\n\s*/g, ' ') : null;
}

// Lines of an HTML block that only collapse a section on GitHub. The recipe
// parser rejects any other raw-HTML line.
const WRAPPER = /^\s*(<details>|<\/details>|<summary>.*<\/summary>)?\s*$/;

/**
 * A markdown-it instance for recipe narratives.
 *
 * - `rewriteHref(href)` turns a link written for GitHub into the site's
 *   href, or null to keep only its text; `siteUrl` marks the site's own
 *   links, which open in the same tab.
 * - Rendering a narrative fills `env.lede` (the markdown of the opening
 *   italic paragraph, which the page shows as its introduction, or null) and
 *   `env.toc` (`[{id, label}]`, the `##` headings as the page lists them),
 *   and renders each fence with `env.fence(token, line)`, `line` being its
 *   0-based line in the narrative. The `# ` heading is left out (the page
 *   header shows it); the narrative has no `## Data` section to leave out.
 */
export function createMarkdown({ rewriteHref, siteUrl }) {
  // `html: true` only so `<details>` wrappers parse as HTML blocks, which are
  // dropped; inline HTML renders as text.
  const md = new MarkdownIt({ html: true, linkify: true, typographer: false });
  // `npm/express@4.18.2` is a package, not an email address.
  md.linkify.set({ fuzzyEmail: false });

  md.core.ruler.push('cookbook_page', (state) => {
    if (state.inlineMode) return;
    const kept = [];
    let seenTitle = false;
    const tokens = state.tokens;
    for (let i = 0; i < tokens.length; i++) {
      const t = tokens[i];
      // The title is the page header's.
      if (t.type === 'heading_open' && t.tag === 'h1' && !seenTitle) {
        seenTitle = true;
        i += 2;
        const lede = tokens[i + 1]?.type === 'paragraph_open' ? italicText(tokens[i + 2].content) : null;
        if (lede !== null) {
          state.env.lede = lede;
          i += 3;
        }
        continue;
      }
      if (t.type === 'html_block') {
        const text = t.content
          .split('\n')
          .filter((line) => !WRAPPER.test(line))
          .join('\n');
        if (!text.trim()) continue;
        t.content = `<p>${md.utils.escapeHtml(text)}</p>\n`;
      }
      for (const child of t.children ?? []) {
        if (child.type === 'html_inline') child.type = 'text';
      }
      kept.push(t);
    }
    state.tokens = kept;
  });

  // Links (markdown and linkified): rewrite each href for the site, open
  // external ones in a new tab, and drop the ones that are not public,
  // keeping their text. CommonMark links do not nest, so one flag pairs open
  // and close.
  md.core.ruler.push('cookbook_links', (state) => {
    for (const block of state.tokens) {
      if (block.type !== 'inline' || !block.children) continue;
      let dropping = false;
      block.children = block.children.filter((t) => {
        if (t.type === 'link_close' && dropping) {
          dropping = false;
          return false;
        }
        if (t.type !== 'link_open') return true;
        const href = rewriteHref(t.attrGet('href'));
        if (href === null) {
          dropping = true;
          return false;
        }
        t.attrSet('href', href);
        if (/^https?:/.test(href) && !href.startsWith(siteUrl)) {
          t.attrSet('target', '_blank');
          t.attrSet('rel', 'noopener');
        }
        return true;
      });
    }
  });

  md.core.ruler.push('cookbook_headings', (state) => {
    if (state.inlineMode) return;
    const used = new Set();
    state.env.toc = [];
    const tokens = state.tokens;
    for (let i = 0; i < tokens.length; i++) {
      const t = tokens[i];
      if (t.type === 'heading_open') {
        const id = slugify(tokens[i + 1].content, used);
        t.attrSet('id', id);
        if (t.tag === 'h2') {
          const label = md.renderer.renderInline(tokens[i + 1].children, md.options, state.env);
          state.env.toc.push({ id, label });
        }
      }
      if (
        t.type === 'blockquote_open' &&
        tokens[i + 1]?.type === 'paragraph_open' &&
        /^\*\*(Note|Tip|Warning)\*\*/.test(tokens[i + 2]?.content ?? '')
      ) {
        t.attrJoin('class', 'callout');
      }
    }
  });

  md.renderer.rules.fence = (tokens, idx, _options, env) => env.fence(tokens[idx], tokens[idx].map[0]);
  return md;
}
