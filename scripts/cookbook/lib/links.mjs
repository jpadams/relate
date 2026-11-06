// Links on the cookbook pages (ADR-0133): how a link written for GitHub
// becomes the site's, and the checks that every link and script import the
// build wrote resolves.
//
// A URL is not public yet while lychee.toml excludes it: that `exclude` list
// is the one switch. The build reads the list it is given (`--lychee`; the
// deploy passes main's, so publication takes effect on the next deploy, not
// the next release). Such links render as their text only, and the build
// fails on any that slips through; publication deletes the entries. Whether
// external URLs are alive is lychee's job, over the recipe markdown (the
// `link-check` CI job).

import { posix } from 'node:path';

import { parse } from 'smol-toml';

import { esc } from './html.mjs';

/**
 * The regular expressions in `lychee.toml` text's top-level `exclude` array.
 * Throws when the text is not TOML or `exclude` is not an array of strings:
 * a list read wrongly would publish the links it is there to hold back.
 */
export function excludePatterns(toml) {
  const { exclude } = parse(toml);
  if (!Array.isArray(exclude) || !exclude.every((p) => typeof p === 'string')) {
    throw new Error('lychee.toml: `exclude` must be a top-level array of strings');
  }
  return exclude.map((p) => new RegExp(p));
}

/**
 * The links policy for a `lychee.toml` text: `{isNotYetPublic, publicHref,
 * publicLink}`.
 */
export function linkPolicy(lycheeToml) {
  const patterns = excludePatterns(lycheeToml);
  /** Whether `url` is not public yet. */
  const isNotYetPublic = (url) => patterns.some((re) => re.test(url));
  /** `url`, or null while it is not public yet. */
  const publicHref = (url) => (isNotYetPublic(url) ? null : url);
  /** An outbound link to `url` (`label` is HTML), or '' while `url` is not public yet. */
  const publicLink = (url, label) => {
    const href = publicHref(url);
    return href ? `<a href="${esc(href)}" target="_blank" rel="noopener">${label}</a>` : '';
  };
  return { isNotYetPublic, publicHref, publicLink };
}

/**
 * The site's href for a link a recipe writes for GitHub (relative to
 * docs/cookbook/), or null when its target is not public: the page keeps
 * the text only. `slugs` are the recipes on the site; other repository files
 * link to `repoUrl` at `ref`. `links` is the policy (linkPolicy).
 */
export function rewriteHref(href, { slugs, repoUrl, ref, links }) {
  if (!href || href.startsWith('#') || href.startsWith('//')) return href;
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return links.publicHref(href);
  const [path, hash = ''] = href.split('#');
  const suffix = hash ? `#${hash}` : '';
  const resolved = posix.normalize(posix.join('docs/cookbook', path));
  if (resolved === 'docs/cookbook' || resolved === 'docs/cookbook/README.md') return `../${suffix}`;
  const m = /^docs\/cookbook\/([^/]+)\.md$/.exec(resolved);
  if (m && slugs.has(m[1])) return `../${m[1]}/${suffix}`;
  return links.publicHref(`${repoUrl}/blob/${ref}/${resolved}${suffix}`);
}

/**
 * Check the `<a href>` links of built pages.
 *
 * - `pages`: Map from a page's path under the cookbook root
 *   (`nearby-places/index.html`) to its HTML.
 * - `files`: Set of every path the build writes, pages included.
 * - `siteUrl`: the site root the pages were built for
 *   (`https://…/relate/`). Links into it, absolute or relative, must
 *   resolve: to a written file under `cookbook/` (and, with a #fragment, to
 *   an element on that page), or to the site's home page or playground.
 * - `links`: the policy (linkPolicy); no link may be to a URL that is not
 *   public yet,
 * - except the URLs in `exempt`, matched exactly (the navigation's
 *   repository link, ADR-0288).
 *
 * Returns one message per broken link.
 */
export function checkLinks(pages, files, { siteUrl, links, exempt = new Set() }) {
  const site = new URL(siteUrl);
  const cookbook = new URL('cookbook/', site);
  const sitePages = new Set([site.pathname, new URL('playground/', site).pathname]);
  const problems = [];
  const ids = new Map();
  const idsOf = (path) => {
    if (!ids.has(path)) {
      ids.set(path, new Set([...(pages.get(path) ?? '').matchAll(/\sid="([^"]+)"/g)].map((m) => m[1])));
    }
    return ids.get(path);
  };
  for (const [path, html] of pages) {
    const base = new URL(path, cookbook);
    for (const m of html.matchAll(/<a\s[^>]*?href="([^"]*)"/g)) {
      const href = m[1].replaceAll('&amp;', '&');
      const where = `${path}: ${href}`;
      if (/^(mailto|tel):/i.test(href)) continue;
      const url = new URL(href, base);
      if (url.origin !== site.origin) {
        if (links.isNotYetPublic(href) && !exempt.has(href)) problems.push(`${where} is not public yet`);
        continue;
      }
      if (!url.pathname.startsWith(site.pathname)) {
        problems.push(`${where} leaves the site`);
        continue;
      }
      const target = decodeURIComponent(url.pathname);
      if (!target.startsWith(cookbook.pathname)) {
        if (!sitePages.has(target)) problems.push(`${where} leaves the cookbook for an unknown page`);
        continue;
      }
      let file = target.slice(cookbook.pathname.length);
      if (file === '' || file.endsWith('/')) file += 'index.html';
      if (!files.has(file)) {
        problems.push(`${where} does not exist`);
        continue;
      }
      const frag = decodeURIComponent(url.hash.slice(1));
      if (frag && file.endsWith('.html') && !idsOf(file).has(frag)) problems.push(`${where} has no #${frag}`);
    }
  }
  return problems;
}

// A relative specifier in `… from './x.js'`, `import './x.js'` or
// `import('./x.js')`, minified or not; not `a.from(…)` or `a.import(…)`.
const IMPORT = /(?<![\w$.])(?:from\s*|import\s*\(?\s*)["'](\.{1,2}\/[^"']+)["']/g;

/**
 * Check the relative imports of built scripts. `files` maps every path the
 * build writes to its contents (text or bytes); each `.js` file's imports of
 * `./` and `../` paths must be among them. One missing file fails the whole
 * module graph in the browser, and a module worker reports it with no
 * message at all.
 *
 * Returns one message per unresolved import.
 */
export function checkImports(files) {
  const problems = [];
  for (const [path, contents] of files) {
    if (!path.endsWith('.js')) continue;
    const text = typeof contents === 'string' ? contents : new TextDecoder().decode(contents);
    for (const [, spec] of text.matchAll(IMPORT)) {
      const target = posix.normalize(posix.join(posix.dirname(path), spec));
      if (!files.has(target)) problems.push(`${path}: ${spec} does not exist`);
    }
  }
  return problems;
}
