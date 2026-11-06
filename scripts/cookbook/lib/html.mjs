// HTML text and attribute escaping, for every page and SVG the cookbook
// writes.

/** `s` with `&`, `<`, `>`, and `"` escaped. */
export const esc = (s) =>
  String(s).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
