//! The generated recipe index in `docs/cookbook/README.md`.
//!
//! The table between [`START`] and [`END`] is regenerated from the recipes'
//! front matter: one section per category (in [`Category::ALL`] order), one
//! row per recipe sorted by title (byte order, as the website sorts).

use crate::frontmatter::{Browser, Category};
use crate::recipe::Recipe;

pub const START: &str = "<!-- cookbook-index:start -->";
pub const END: &str = "<!-- cookbook-index:end -->";

/// The index markdown for `recipes` (without the markers).
pub fn render(recipes: &[Recipe]) -> String {
    let mut out = String::new();
    for category in Category::ALL {
        let mut rows: Vec<&Recipe> = recipes
            .iter()
            .filter(|r| r.front.category == category)
            .collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by(|a, b| a.front.title.cmp(&b.front.title));
        out.push_str(&format!(
            "\n### {}\n\n| Recipe | What it does | Level | Runs in the browser |\n|---|---|---|---|\n",
            category.name()
        ));
        for r in rows {
            let browser = match r.front.browser {
                Browser::Full => "Fully",
                Browser::Partial => "Cypher steps",
            };
            out.push_str(&format!(
                "| [{}]({}.md) | {} | {} | {} |\n",
                r.front.title,
                r.slug,
                r.front.summary.replace('|', "\\|"),
                r.front.level.name(),
                browser
            ));
        }
    }
    out.push('\n');
    out
}
/// `readme` with the index section replaced by `index`, or an error when
/// the markers are missing.
pub fn splice(readme: &str, index: &str) -> Result<String, String> {
    let start = readme
        .find(START)
        .ok_or_else(|| format!("docs/cookbook/README.md lacks `{START}`"))?
        + START.len();
    let end = readme
        .find(END)
        .ok_or_else(|| format!("docs/cookbook/README.md lacks `{END}`"))?;
    if end < start {
        return Err("cookbook index markers are out of order".into());
    }
    Ok(format!("{}{}{}", &readme[..start], index, &readme[end..]))
}
