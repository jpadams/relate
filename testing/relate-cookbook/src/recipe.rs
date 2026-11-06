//! Recipe parsing (ADR-0130, ADR-0132): front matter, runnable cells, data
//! blocks, diagrams, and each cell's recorded result.
//!
//! A recipe is one markdown file. Fences are recognised line by line at
//! column 0 (```` ``` ```` or `~~~`, three or more):
//!
//! - ```` ```cypher cell=<id> … ```` is a runnable cell;
//! - ```` ```json data=<name> ```` is a data block: a JSON object whose keys
//!   are statement parameters, named by a cell's `params=<name>`;
//! - ```` ```table ```` or ```` ```gram ```` directly after a cell (blank lines
//!   between are fine) is that cell's recorded result;
//! - a fence whose language is `mermaid` is a diagram (ADR-0135).
//!
//! Every other fence is code. The only raw HTML allowed is a line holding
//! `<details>`, `</details>`, or `<summary>…</summary>`, which collapses the
//! data section on GitHub. This is the one recipe parser: the Node tools read
//! its output through `cookbook-export` ([`crate::export`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use relate_formatter::recording::{Kind, Recording};

use crate::frontmatter::{self, FrontMatter};

/// A parsed recipe.
#[derive(Debug, Clone)]
pub struct Recipe {
    /// File stem, which is also the URL slug.
    pub slug: String,
    /// The recipe file.
    pub path: PathBuf,
    /// The file as read.
    pub source: String,
    pub front: FrontMatter,
    /// 1-based line where the body (everything after the front matter) starts.
    pub body_line: usize,
    /// Byte offset where the body starts.
    body_start: usize,
    /// The first `# ` heading after the front matter.
    pub heading: Option<String>,
    pub cells: Vec<Cell>,
    pub data: Vec<DataBlock>,
    /// The ```` ```mermaid ```` blocks, in order (ADR-0135).
    pub diagrams: Vec<Diagram>,
    /// Every fence, in order, with what it is.
    pub fences: Vec<FenceRef>,
    /// Raw HTML lines other than the allowed `<details>` wrappers.
    pub stray_html: Vec<usize>,
    /// Lines before the `## Data` heading (the whole file when it has none).
    pub narrative_lines: usize,
    /// Byte offset where the `## Data` section starts (the end of the file
    /// when it has none).
    narrative_end: usize,
}

/// One runnable ```` ```cypher cell=<id> ```` fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub id: String,
    /// The fence body (the Cypher script, possibly several statements).
    pub code: String,
    /// `setup`: collapsed on the website; still runs in order.
    pub setup: bool,
    /// `params=<name>`: the data block passed verbatim as the parameters.
    pub params: Option<String>,
    /// `expect=error`: the cell must fail; its result shows the error.
    pub expect_error: bool,
    /// 1-based line of the opening fence.
    pub line: usize,
    /// Byte offset just past the cell's closing fence line.
    pub end: usize,
    /// The recorded result block, if present.
    pub result: Option<ResultBlock>,
}

/// A ```` ```table ```` / ```` ```gram ```` block: kind, byte span (fences
/// included), and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultBlock {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// A ```` ```json data=<name> ```` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBlock {
    pub name: String,
    /// The block body, passed to the engine as the parameters' JSON text.
    pub text: String,
    pub line: usize,
}

/// A ```` ```mermaid ```` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagram {
    /// 1-based position among the recipe's diagrams.
    pub index: usize,
    pub source: String,
    pub line: usize,
}

/// What a fence is, by the 1-based line of its opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FenceRef {
    pub line: usize,
    pub kind: FenceKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FenceKind {
    /// A runnable cell, by id.
    Cell(String),
    /// The result block of the cell with this id.
    Result(String),
    /// A data block, by name.
    Data(String),
    /// A diagram, by index.
    Diagram(usize),
    /// Any other fence: code shown as is.
    Code,
}

struct Line<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn lines_with_offsets(s: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    for piece in s.split_inclusive('\n') {
        let end = start + piece.len();
        out.push(Line {
            start,
            end,
            text: piece.trim_end_matches('\n').trim_end_matches('\r'),
        });
        start = end;
    }
    out
}

/// `(fence char, fence length, info string)` for an opening fence line.
fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    let ch = line.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = line.chars().take_while(|&c| c == ch).count();
    if len < 3 {
        return None;
    }
    let info = &line[len..];
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, len, info.trim()))
}

fn is_fence_close(line: &str, ch: char, len: usize) -> bool {
    let run = line.chars().take_while(|&c| c == ch).count();
    run >= len && line[run..].trim().is_empty()
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A raw-HTML line (one starting with `<` followed by a letter, `/`, or
/// `!`) other than the allowed `<details>` wrappers.
fn is_stray_html(text: &str) -> bool {
    let t = text.trim();
    let mut chars = t.chars();
    let html = chars.next() == Some('<')
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!');
    let allowed = t == "<details>"
        || t == "</details>"
        || (t.starts_with("<summary>") && t.ends_with("</summary>"));
    html && !allowed
}

/// A cell fence's options.
struct CellOptions {
    id: String,
    setup: bool,
    params: Option<String>,
    expect_error: bool,
}

enum Fence {
    Cell(CellOptions),
    Data(String),
    Result(Kind),
    Diagram,
    Code,
}

/// Classify a fence by its info string.
fn classify(info: &str, line: usize) -> Result<Fence, String> {
    if let Some(kind) = Kind::from_info(info) {
        return Ok(Fence::Result(kind));
    }
    let mut tokens = info.split_whitespace();
    match tokens.next() {
        // A diagram: the fence's language is mermaid, as markdown-it and
        // GitHub read it, whatever else the info string holds.
        Some("mermaid") => Ok(Fence::Diagram),
        Some("json") => match tokens.collect::<Vec<_>>().as_slice() {
            [] => Ok(Fence::Code),
            [tok] if tok.strip_prefix("data=").is_some_and(is_name) => {
                Ok(Fence::Data(tok["data=".len()..].to_string()))
            }
            _ => Err(format!(
                "line {line}: expected `json data=<name>`, got `{info}`"
            )),
        },
        Some("cypher") => {
            let rest: Vec<&str> = tokens.collect();
            if rest.is_empty() {
                return Ok(Fence::Code);
            }
            let mut options = CellOptions {
                id: String::new(),
                setup: false,
                params: None,
                expect_error: false,
            };
            for token in rest {
                match token.split_once('=') {
                    Some(("cell", id)) => options.id = id.to_string(),
                    Some(("params", name)) if is_name(name) => {
                        options.params = Some(name.to_string())
                    }
                    Some(("expect", "error")) => options.expect_error = true,
                    None if token == "setup" => options.setup = true,
                    _ => return Err(format!("line {line}: unknown cell option `{token}`")),
                }
            }
            if !is_name(&options.id) {
                return Err(format!(
                    "line {line}: a cypher fence with options needs `cell=<id>` \
                     (lowercase letters, digits, `-`)"
                ));
            }
            Ok(Fence::Cell(options))
        }
        _ => Ok(Fence::Code),
    }
}

impl Recipe {
    /// Read and parse a recipe file.
    pub fn load(path: &Path) -> Result<Recipe, String> {
        let source =
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("{}: bad file name", path.display()))?
            .to_string();
        Self::parse(&slug, path, source)
    }

    /// Parse recipe text.
    pub fn parse(slug: &str, path: &Path, source: String) -> Result<Recipe, String> {
        let lines = lines_with_offsets(&source);
        if lines.first().map(|l| l.text) != Some("---") {
            return Err("the recipe must start with a `---` front-matter block".into());
        }
        let close = lines
            .iter()
            .skip(1)
            .position(|l| l.text == "---")
            .map(|i| i + 1)
            .ok_or("front matter is not closed with `---`")?;
        let front_text: String = lines[1..close]
            .iter()
            .map(|l| format!("{}\n", l.text))
            .collect();
        let front = frontmatter::parse(&front_text)?;

        let mut heading = None;
        let mut cells: Vec<Cell> = Vec::new();
        let mut data: Vec<DataBlock> = Vec::new();
        let mut diagrams: Vec<Diagram> = Vec::new();
        let mut fences: Vec<FenceRef> = Vec::new();
        let mut stray_html = Vec::new();
        let mut narrative_lines = lines.len();
        // Index of the last cell, while nothing but blank lines follows it.
        let mut open_cell: Option<usize> = None;
        let mut i = close + 1;
        while i < lines.len() {
            let line = &lines[i];
            let Some((ch, len, info)) = fence_open(line.text) else {
                if heading.is_none() {
                    if let Some(h) = line.text.strip_prefix("# ") {
                        heading = Some(h.trim().to_string());
                    }
                }
                if line.text == "## Data" && narrative_lines == lines.len() {
                    narrative_lines = i;
                }
                if is_stray_html(line.text) {
                    stray_html.push(i + 1);
                }
                if !line.text.trim().is_empty() {
                    open_cell = None;
                }
                i += 1;
                continue;
            };
            let open = i + 1;
            let mut j = i + 1;
            while j < lines.len() && !is_fence_close(lines[j].text, ch, len) {
                j += 1;
            }
            if j == lines.len() {
                return Err(format!("line {open}: unclosed code fence"));
            }
            let body: String = lines[i + 1..j]
                .iter()
                .map(|l| format!("{}\n", l.text))
                .collect();
            let body = body.trim_end_matches('\n').to_string();
            // What the fence is, and the cell a result block may follow next.
            let (kind, next_open) = match classify(info, open)? {
                Fence::Result(kind) => {
                    let idx = open_cell.ok_or_else(|| {
                        format!(
                            "line {open}: a `{}` result block must directly follow a cypher cell",
                            kind.info()
                        )
                    })?;
                    cells[idx].result = Some(ResultBlock {
                        kind,
                        start: lines[i].start,
                        end: lines[j].end,
                        text: body,
                    });
                    (FenceKind::Result(cells[idx].id.clone()), None)
                }
                Fence::Cell(options) => {
                    if cells.iter().any(|c| c.id == options.id) {
                        return Err(format!("line {open}: duplicate cell id `{}`", options.id));
                    }
                    cells.push(Cell {
                        id: options.id.clone(),
                        code: body,
                        setup: options.setup,
                        params: options.params,
                        expect_error: options.expect_error,
                        line: open,
                        end: lines[j].end,
                        result: None,
                    });
                    (FenceKind::Cell(options.id), Some(cells.len() - 1))
                }
                Fence::Data(name) => {
                    if data.iter().any(|d| d.name == name) {
                        return Err(format!("line {open}: duplicate data block `{name}`"));
                    }
                    data.push(DataBlock {
                        name: name.clone(),
                        text: body,
                        line: open,
                    });
                    (FenceKind::Data(name), None)
                }
                Fence::Diagram => {
                    let index = diagrams.len() + 1;
                    diagrams.push(Diagram {
                        index,
                        source: body,
                        line: open,
                    });
                    (FenceKind::Diagram(index), None)
                }
                Fence::Code => (FenceKind::Code, None),
            };
            fences.push(FenceRef { line: open, kind });
            open_cell = next_open;
            i = j + 1;
        }

        let body_start = lines[close].end;
        let narrative_end = lines.get(narrative_lines).map_or(source.len(), |l| l.start);
        Ok(Recipe {
            slug: slug.to_string(),
            path: path.to_path_buf(),
            front,
            body_line: close + 2,
            body_start,
            heading,
            cells,
            data,
            diagrams,
            fences,
            stray_html,
            narrative_lines,
            narrative_end,
            source,
        })
    }

    /// The recipe as a reader reads it: everything after the front matter,
    /// up to the `## Data` section. Everything from `## Data` on is data.
    pub fn narrative(&self) -> &str {
        &self.source[self.body_start..self.narrative_end]
    }

    /// The data block named `name`.
    pub fn data_block(&self, name: &str) -> Option<&DataBlock> {
        self.data.iter().find(|d| d.name == name)
    }

    /// Contract checks beyond the front matter's own schema (which parsing
    /// enforces), none needing the engine: heading, cells, a diagram, data
    /// blocks, step code width, raw HTML, narrative length. Returns every
    /// problem found.
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        match &self.heading {
            Some(h) if *h != self.front.title => errs.push(format!(
                "first `# ` heading `{h}` differs from title `{}`",
                self.front.title
            )),
            None => errs.push("recipe has no `# ` heading".into()),
            _ => {}
        }
        if self.cells.is_empty() {
            errs.push("recipe has no runnable `cypher cell=<id>` fences".into());
        }
        // Every recipe draws at least its data model (ADR-0135).
        if self.diagrams.is_empty() {
            errs.push("recipe has no ```mermaid diagram (draw its data model)".into());
        }
        let ships_dataset = self
            .data
            .iter()
            .any(|d| d.text.len() > crate::INLINE_DATA_BYTES);
        if ships_dataset {
            let fm = &self.front;
            for (key, value) in [
                ("dataset", &fm.dataset),
                ("source", &fm.source),
                ("license", &fm.license),
            ] {
                if value.is_none() {
                    errs.push(format!(
                        "recipe ships a dataset but front matter lacks `{key}`"
                    ));
                }
            }
        }
        for cell in &self.cells {
            if let Some(p) = &cell.params {
                if self.data_block(p).is_none() {
                    errs.push(format!("cell `{}`: no `json data={p}` block", cell.id));
                }
            }
            if relate_splitter::split_executable_statements(&cell.code).is_empty() {
                errs.push(format!("cell `{}` has no statements", cell.id));
            }
            for (n, line) in cell.code.lines().enumerate() {
                let width = line.chars().count();
                if width > crate::MAX_CODE_COLUMNS {
                    errs.push(format!(
                        "cell `{}` line {} is {width} columns; wrap Cypher at {}",
                        cell.id,
                        n + 1,
                        crate::MAX_CODE_COLUMNS
                    ));
                }
            }
        }
        for d in &self.data {
            if !self
                .cells
                .iter()
                .any(|c| c.params.as_deref() == Some(&d.name))
            {
                errs.push(format!("data block `{}` is not used by any cell", d.name));
            }
            // Parameter blocks sit beside their step and count toward the
            // narrative; only datasets go in the `## Data` section.
            let dataset = d.text.len() > crate::INLINE_DATA_BYTES;
            let in_data_section = d.line > self.narrative_lines;
            if in_data_section && !dataset {
                errs.push(format!(
                    "data block `{}` is {} bytes, a parameter block: put it beside the step that \
                     uses it; `## Data` holds datasets (over {} bytes)",
                    d.name,
                    d.text.len(),
                    crate::INLINE_DATA_BYTES
                ));
            } else if dataset && !in_data_section {
                errs.push(format!(
                    "data block `{}` is a {} KB dataset: move it to the `## Data` section",
                    d.name,
                    d.text.len() / 1024
                ));
            }
            if d.text.len() > crate::MAX_DATA_BYTES {
                errs.push(format!(
                    "data block `{}` is {} KB; blocks are capped at {} KB",
                    d.name,
                    d.text.len() / 1024,
                    crate::MAX_DATA_BYTES / 1024
                ));
            }
            match serde_json::from_str::<serde_json::Value>(&d.text) {
                Ok(serde_json::Value::Object(_)) => {}
                Ok(_) => errs.push(format!("data block `{}` must be a JSON object", d.name)),
                Err(e) => errs.push(format!("data block `{}` is not JSON: {e}", d.name)),
            }
        }
        for line in &self.stray_html {
            errs.push(format!(
                "line {line}: raw HTML is not allowed (only `<details>` / `<summary>` lines)"
            ));
        }
        if self.narrative_lines > crate::MAX_NARRATIVE_LINES {
            errs.push(format!(
                "the recipe runs {} lines before `## Data`; keep it under {} \
                 (datasets go in the `## Data` section)",
                self.narrative_lines,
                crate::MAX_NARRATIVE_LINES
            ));
        }
        errs
    }

    /// The recipe with every cell's result block set to its recording in
    /// `recordings` (by cell id), inserted after the cell when missing.
    pub fn with_results(&self, recordings: &BTreeMap<String, Recording>) -> String {
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for cell in &self.cells {
            let Some(rec) = recordings.get(&cell.id) else {
                continue;
            };
            let fence = if rec.text.contains("```") {
                "````"
            } else {
                "```"
            };
            let block = format!("{fence}{}\n{}\n{fence}\n", rec.kind().info(), rec.text);
            match &cell.result {
                Some(out) => edits.push((out.start, out.end, block)),
                None => edits.push((cell.end, cell.end, format!("\n{block}"))),
            }
        }
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        let mut s = self.source.clone();
        for (start, end, text) in edits {
            s.replace_range(start..end, &text);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use relate_formatter::recording::View;

    use super::*;

    const SAMPLE: &str = "---\ntitle: T\nsummary: S\nlevel: Beginner\ncategory: Geospatial\nfeatures: [point]\nbrowser: full\n---\n\n# T\n\nIntro.\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n```cypher cell=load setup params=rows\nUNWIND $rows AS r\nCREATE (:P {id: r.id})\n```\n\n```table\nold\n```\n\n```cypher\nMATCH (n) RETURN n // static, not a cell\n```\n\n```json data=k\n{\"k\": 1}\n```\n\n```cypher cell=q params=k\nMATCH (p:P) RETURN p LIMIT $k\n```\n\nText after.\n\n## Data\n\n<details>\n<summary><code>rows</code></summary>\n\n```json data=rows\n{\"rows\": [{\"id\": 1}]}\n```\n\n</details>\n";

    fn parse(s: &str) -> Result<Recipe, String> {
        Recipe::parse("t", Path::new("/nowhere/t.md"), s.to_string())
    }

    #[test]
    fn parses_cells_data_and_results() {
        let r = parse(SAMPLE).unwrap();
        assert_eq!(r.heading.as_deref(), Some("T"));
        assert_eq!(r.cells.len(), 2);
        let load = &r.cells[0];
        assert_eq!(load.id, "load");
        assert!(load.setup);
        assert_eq!(load.params.as_deref(), Some("rows"));
        assert_eq!(load.code, "UNWIND $rows AS r\nCREATE (:P {id: r.id})");
        let res = load.result.as_ref().unwrap();
        assert_eq!((res.kind, res.text.as_str()), (Kind::Table, "old"));
        assert!(r.cells[1].result.is_none());
        let names: Vec<&str> = r.data.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["k", "rows"]);
        assert_eq!(
            r.data_block("rows").unwrap().text,
            "{\"rows\": [{\"id\": 1}]}"
        );
        assert!(r.stray_html.is_empty());
        assert!(r.narrative().starts_with("\n# T\n"));
        assert!(r.narrative().ends_with("Text after.\n\n"));
        assert_eq!(r.body_line, 9);
        let kinds: Vec<(usize, FenceKind)> =
            r.fences.iter().map(|f| (f.line, f.kind.clone())).collect();
        assert_eq!(
            kinds,
            [
                (14, FenceKind::Diagram(1)),
                (19, FenceKind::Cell("load".into())),
                (24, FenceKind::Result("load".into())),
                (28, FenceKind::Code),
                (32, FenceKind::Data("k".into())),
                (36, FenceKind::Cell("q".into())),
                (47, FenceKind::Data("rows".into())),
            ]
        );
        assert_eq!(r.narrative_lines, 41);
    }

    fn recording(kind: Kind, text: &str) -> Recording {
        let view = match kind {
            Kind::Table => View::Table(None),
            Kind::Gram => View::Gram(Default::default()),
        };
        Recording {
            text: text.into(),
            view,
            stats: Vec::new(),
        }
    }

    #[test]
    fn rewrites_and_inserts_results() {
        let r = parse(SAMPLE).unwrap();
        let s = r.with_results(&BTreeMap::from([
            ("load".to_string(), recording(Kind::Table, "(no rows)")),
            (
                "q".to_string(),
                recording(Kind::Gram, "// 1 row\n(n0:P {id: 1})"),
            ),
        ]));
        assert!(s.contains("```table\n(no rows)\n```\n"));
        assert!(s.contains("LIMIT $k\n```\n\n```gram\n// 1 row\n(n0:P {id: 1})\n```\n"));
        let again = parse(&s).unwrap();
        assert_eq!(again.cells[1].result.as_ref().unwrap().kind, Kind::Gram);
    }

    #[test]
    fn results_must_follow_a_cell_and_options_are_strict() {
        let bad = SAMPLE.replace("Text after.", "Text.\n\n```gram\nx\n```");
        assert!(parse(&bad).unwrap_err().contains("must directly follow"));
        let bad = SAMPLE.replace("params=k", "view=graph");
        assert!(parse(&bad).unwrap_err().contains("unknown cell option"));
        let bad = SAMPLE.replace("cell=q", "cell=load");
        assert!(parse(&bad).unwrap_err().contains("duplicate cell id"));
        let bad = SAMPLE.replace("json data=k", "json data=rows");
        assert!(parse(&bad).unwrap_err().contains("duplicate data block"));
        let bad = SAMPLE.replace("json data=k", "json data=K");
        assert!(parse(&bad)
            .unwrap_err()
            .contains("expected `json data=<name>`"));
    }

    #[test]
    fn validation_catches_missing_and_unused_data_and_html() {
        let r = parse(&SAMPLE.replace("params=k", "params=nope")).unwrap();
        let errs = r.validate().join("\n");
        assert!(errs.contains("no `json data=nope` block"), "{errs}");
        assert!(errs.contains("data block `k` is not used"), "{errs}");
        let r = parse(&SAMPLE.replace("Intro.", "Intro.\n\n<div>hi</div>")).unwrap();
        assert!(r.validate().join("\n").contains("raw HTML"));
        assert!(!is_stray_html("<summary><code>rows</code></summary>"));
        assert!(!is_stray_html("a < b"));
        let r =
            parse(&SAMPLE.replace("MATCH (p:P) RETURN p LIMIT $k", "// nothing to run")).unwrap();
        assert!(r
            .validate()
            .join("\n")
            .contains("cell `q` has no statements"));
    }

    #[test]
    fn validation_caps_the_narrative_but_not_the_data() {
        let filler = "Prose.\n".repeat(crate::MAX_NARRATIVE_LINES);
        let long = parse(&SAMPLE.replace("Text after.", &filler)).unwrap();
        assert!(long
            .validate()
            .join("\n")
            .contains("lines before `## Data`"));
        let data = format!(
            "\n{{\"rows\": [\n{}{{\"id\": 1}}]}}\n",
            "{\"id\": 0},\n".repeat(900)
        );
        let big = parse(&SAMPLE.replace("\n{\"rows\": [{\"id\": 1}]}\n", &data)).unwrap();
        assert!(big.source.lines().count() > 900);
        assert!(!big.validate().join("\n").contains("lines before"));
    }

    #[test]
    fn diagrams_are_every_fence_whose_language_is_mermaid() {
        let r = parse(&SAMPLE.replace(
            "Text after.",
            "Text after.\n\n~~~mermaid\nflowchart TD\n  C --> D\n~~~\n\n````mermaid accTitle\ngantt\n````\n\n```text\nmermaid\n```",
        ))
        .unwrap();
        let sources: Vec<&str> = r.diagrams.iter().map(|d| d.source.as_str()).collect();
        assert_eq!(
            sources,
            [
                "flowchart LR\n  A --> B",
                "flowchart TD\n  C --> D",
                "gantt"
            ]
        );
        assert_eq!(r.diagrams[2].index, 3);
    }

    #[test]
    fn validation_requires_a_diagram() {
        let r = parse(&SAMPLE.replace("```mermaid\nflowchart LR\n  A --> B\n```\n\n", "")).unwrap();
        assert!(r.diagrams.is_empty());
        assert!(r.validate().join("\n").contains("no ```mermaid diagram"));
    }

    #[test]
    fn validation_caps_the_width_of_step_code() {
        let wide = format!("RETURN 1 AS {}", "x".repeat(80));
        let r = parse(&SAMPLE.replace("MATCH (p:P) RETURN p LIMIT $k", &wide)).unwrap();
        let errs = r.validate().join("\n");
        assert!(errs.contains("cell `q` line 1 is 92 columns"), "{errs}");
        assert!(parse(&params_beside_steps()).unwrap().validate().is_empty());
    }

    /// SAMPLE with its small `rows` block beside the step that uses it.
    fn params_beside_steps() -> String {
        let block = "```json data=rows\n{\"rows\": [{\"id\": 1}]}\n```\n";
        let section = format!(
            "\n## Data\n\n<details>\n<summary><code>rows</code></summary>\n\n{block}\n</details>\n"
        );
        SAMPLE.replace(&section, "").replace(
            "```cypher cell=load",
            &format!("{block}\n```cypher cell=load"),
        )
    }

    #[test]
    fn validation_keeps_parameters_beside_steps_and_datasets_in_data() {
        let errs = parse(SAMPLE).unwrap().validate().join("\n");
        assert!(
            errs.contains("data block `rows` is 21 bytes, a parameter block"),
            "{errs}"
        );
        let dataset = format!(
            "{{\"k\": 1, \"pad\": \"{}\"}}",
            "x".repeat(crate::INLINE_DATA_BYTES)
        );
        let r = parse(&params_beside_steps().replace("{\"k\": 1}", &dataset)).unwrap();
        let errs = r.validate().join("\n");
        assert!(
            errs.contains("data block `k` is a 2 KB dataset: move it to the `## Data` section"),
            "{errs}"
        );
    }
}
