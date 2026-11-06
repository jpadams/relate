//! Recipe front matter: a deliberately small YAML subset, parsed into a
//! typed [`FrontMatter`].
//!
//! One `key: value` per line. A value is a plain scalar, a `"double"` or
//! `'single'` quoted scalar, or a flow list `[a, "b, c", d]`. No nesting, no
//! comments, no multi-line values. Parsing checks the whole schema: the keys,
//! the required ones, the enumerated values, and the value rules, so a
//! [`FrontMatter`] is always a valid one.

use std::collections::BTreeMap;

/// Front-matter keys a recipe may use, in the order the docs list them.
pub const KEYS: &[&str] = &[
    "title",
    "summary",
    "level",
    "category",
    "features",
    "browser",
    "dataset",
    "source",
    "license",
    "companion",
];

/// Longest `summary`, in characters.
pub const MAX_SUMMARY_CHARS: usize = 200;

/// How hard a recipe is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Beginner,
    Intermediate,
    Advanced,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Beginner, Level::Intermediate, Level::Advanced];

    pub fn name(self) -> &'static str {
        match self {
            Level::Beginner => "Beginner",
            Level::Intermediate => "Intermediate",
            Level::Advanced => "Advanced",
        }
    }
}

/// The section a recipe is listed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    AgentMemory,
    SearchAndRetrieval,
    DataQuality,
    GraphAnalytics,
    Geospatial,
}

impl Category {
    /// Every category, in index order.
    pub const ALL: [Category; 5] = [
        Category::AgentMemory,
        Category::SearchAndRetrieval,
        Category::DataQuality,
        Category::GraphAnalytics,
        Category::Geospatial,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::AgentMemory => "Agent memory",
            Category::SearchAndRetrieval => "Search and retrieval",
            Category::DataQuality => "Data quality",
            Category::GraphAnalytics => "Graph analytics",
            Category::Geospatial => "Geospatial",
        }
    }
}

/// How much of a recipe runs on the website.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    /// Every step runs in the page.
    Full,
    /// Some steps (Python, an LLM call) are shown as code only.
    Partial,
}

impl Browser {
    pub const ALL: [Browser; 2] = [Browser::Full, Browser::Partial];

    pub fn name(self) -> &'static str {
        match self {
            Browser::Full => "full",
            Browser::Partial => "partial",
        }
    }
}

/// A recipe's front matter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatter {
    pub title: String,
    /// At most [`MAX_SUMMARY_CHARS`] characters.
    pub summary: String,
    pub level: Level,
    pub category: Category,
    /// Non-empty.
    pub features: Vec<String>,
    pub browser: Browser,
    /// Where a shipped dataset comes from; required when the recipe ships one.
    pub dataset: Option<String>,
    pub source: Option<String>,
    pub license: Option<String>,
    /// The companion program, an `https://` URL.
    pub companion: Option<String>,
}

impl FrontMatter {
    /// The front matter as JSON, with enums as their names.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "title": self.title,
            "summary": self.summary,
            "level": self.level.name(),
            "category": self.category.name(),
            "features": self.features,
            "browser": self.browser.name(),
            "dataset": self.dataset,
            "source": self.source,
            "license": self.license,
            "companion": self.companion,
        })
    }
}

enum Raw {
    Scalar(String),
    List(Vec<String>),
}

/// Parse front-matter lines (the text between the two `---` lines). Every
/// problem found is reported, one per line.
pub fn parse(text: &str) -> Result<FrontMatter, String> {
    let raw = parse_raw(text)?;
    let mut errs = Vec::new();
    let mut scalar = |key: &str| -> Option<String> {
        match raw.get(key) {
            Some(Raw::Scalar(s)) => Some(s.clone()),
            Some(Raw::List(_)) => {
                errs.push(format!("`{key}` must be a single value, not a list"));
                None
            }
            None => None,
        }
    };
    let title = scalar("title");
    let summary = scalar("summary");
    let level = scalar("level");
    let category = scalar("category");
    let browser = scalar("browser");
    let dataset = scalar("dataset");
    let source = scalar("source");
    let license = scalar("license");
    let companion = scalar("companion");
    let features = match raw.get("features") {
        Some(Raw::List(items)) if !items.is_empty() => Some(items.clone()),
        Some(_) => {
            errs.push("`features` must be a non-empty list".into());
            None
        }
        None => None,
    };

    let mut required = |key: &str, value: Option<String>| {
        if value.is_none() && !raw.contains_key(key) {
            errs.push(format!("front matter is missing `{key}`"));
        }
        value
    };
    let title = required("title", title);
    let summary = required("summary", summary);
    let level = required("level", level);
    let category = required("category", category);
    let browser = required("browser", browser);
    if features.is_none() && !raw.contains_key("features") {
        errs.push("front matter is missing `features`".into());
    }

    let level = level.and_then(|v| one_of("level", &v, &Level::ALL, |l| l.name(), &mut errs));
    let category =
        category.and_then(|v| one_of("category", &v, &Category::ALL, |c| c.name(), &mut errs));
    let browser =
        browser.and_then(|v| one_of("browser", &v, &Browser::ALL, |b| b.name(), &mut errs));
    if summary
        .as_ref()
        .is_some_and(|s| s.chars().count() > MAX_SUMMARY_CHARS)
    {
        errs.push(format!(
            "`summary` is longer than {MAX_SUMMARY_CHARS} characters"
        ));
    }
    if companion
        .as_ref()
        .is_some_and(|url| !url.starts_with("https://"))
    {
        errs.push("`companion` must be an https:// URL".into());
    }

    match (title, summary, level, category, features, browser) {
        (
            Some(title),
            Some(summary),
            Some(level),
            Some(category),
            Some(features),
            Some(browser),
        ) if errs.is_empty() => Ok(FrontMatter {
            title,
            summary,
            level,
            category,
            features,
            browser,
            dataset,
            source,
            license,
            companion,
        }),
        _ => Err(errs.join("\n")),
    }
}

fn one_of<T: Copy>(
    key: &str,
    value: &str,
    all: &[T],
    name: impl Fn(T) -> &'static str,
    errs: &mut Vec<String>,
) -> Option<T> {
    let found = all.iter().copied().find(|v| name(*v) == value);
    if found.is_none() {
        let names: Vec<&str> = all.iter().map(|v| name(*v)).collect();
        errs.push(format!("`{key}: {value}` is not one of {names:?}"));
    }
    found
}

fn parse_raw(text: &str) -> Result<BTreeMap<String, Raw>, String> {
    let mut map = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, raw) = line
            .split_once(':')
            .ok_or_else(|| format!("front matter line {}: expected `key: value`", i + 1))?;
        let key = key.trim();
        if !KEYS.contains(&key) {
            return Err(format!("front matter line {}: unknown key `{key}`", i + 1));
        }
        let raw = raw.trim();
        let value = if let Some(inner) = raw.strip_prefix('[') {
            let inner = inner
                .strip_suffix(']')
                .ok_or_else(|| format!("front matter `{key}`: unterminated list"))?;
            Raw::List(split_list(inner).map_err(|e| format!("front matter `{key}`: {e}"))?)
        } else {
            Raw::Scalar(unquote(raw).map_err(|e| format!("front matter `{key}`: {e}"))?)
        };
        if map.insert(key.to_string(), value).is_some() {
            return Err(format!("front matter: duplicate key `{key}`"));
        }
    }
    Ok(map)
}

fn split_list(inner: &str) -> Result<Vec<String>, String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in inner.chars() {
        if let Some(q) = quote {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' && q == '"' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
            current.push(ch);
        } else if ch == ',' {
            items.push(unquote(current.trim())?);
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if quote.is_some() {
        return Err("unterminated quote in list".into());
    }
    if !current.trim().is_empty() {
        items.push(unquote(current.trim())?);
    }
    if items.iter().any(|s| s.is_empty()) {
        return Err("empty list item".into());
    }
    Ok(items)
}

fn unquote(raw: &str) -> Result<String, String> {
    if let Some(body) = raw.strip_prefix('"') {
        let body = body
            .strip_suffix('"')
            .ok_or_else(|| format!("unterminated string {raw}"))?;
        let mut out = String::new();
        let mut chars = body.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    other => return Err(format!("unsupported escape \\{other:?} in {raw}")),
                }
            } else {
                out.push(ch);
            }
        }
        Ok(out)
    } else if let Some(body) = raw.strip_prefix('\'') {
        body.strip_suffix('\'')
            .map(str::to_string)
            .ok_or_else(|| format!("unterminated string {raw}"))
    } else {
        Ok(raw.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "title: Nearby places\nsummary: \"Finds \\\"close\\\" things\"\nfeatures: [point, 'a, b', \"c\"]\nlevel: Beginner\ncategory: Geospatial\nbrowser: full\n";

    #[test]
    fn parses_scalars_lists_and_enums() {
        let fm = parse(BASE).unwrap();
        assert_eq!(fm.title, "Nearby places");
        assert_eq!(fm.summary, "Finds \"close\" things");
        assert_eq!(fm.features, ["point", "a, b", "c"]);
        assert_eq!(fm.level, Level::Beginner);
        assert_eq!(fm.category, Category::Geospatial);
        assert_eq!(fm.browser, Browser::Full);
        assert_eq!(fm.companion, None);
        assert_eq!(fm.to_json()["category"], "Geospatial");
    }

    #[test]
    fn rejects_unknown_and_duplicate_keys() {
        assert!(parse("titel: x").unwrap_err().contains("unknown key"));
        assert!(parse("title: a\ntitle: b")
            .unwrap_err()
            .contains("duplicate"));
        assert!(parse("features: [a, b")
            .unwrap_err()
            .contains("unterminated"));
        assert!(parse("features: [a, , b]")
            .unwrap_err()
            .contains("empty list item"));
        assert!(parse("title: \"a\\nb\"")
            .unwrap_err()
            .contains("unsupported escape"));
    }

    #[test]
    fn reports_every_schema_problem() {
        let errs =
            parse("title: T\nlevel: Expert\nfeatures: []\ncompanion: http://x\n").unwrap_err();
        for want in [
            "missing `summary`",
            "missing `category`",
            "missing `browser`",
            "`level: Expert` is not one of",
            "`features` must be a non-empty list",
            "https:// URL",
        ] {
            assert!(errs.contains(want), "{want} not in:\n{errs}");
        }
        let long = BASE.replace("\"Finds \\\"close\\\" things\"", &"x".repeat(201));
        assert!(parse(&long).unwrap_err().contains("longer than 200"));
    }

    #[test]
    fn keeps_colons_in_values() {
        let fm = parse(&format!("{BASE}companion: https://example.com/a#b\n")).unwrap();
        assert_eq!(fm.companion.as_deref(), Some("https://example.com/a#b"));
    }
}
