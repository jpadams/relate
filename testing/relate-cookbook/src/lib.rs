//! The cookbook harness (ADR-0130, ADR-0132).
//!
//! A recipe is one markdown file, `docs/cookbook/<slug>.md`: YAML-style front
//! matter, prose, runnable ```` ```cypher cell=<id> ```` fences, the
//! ```` ```json data=<name> ```` blocks they take their parameters from, and
//! after each cell a ```` ```table ```` or ```` ```gram ```` block holding its
//! recorded result. This crate parses recipes ([`recipe`], the one recipe
//! parser; [`export`] hands the parse to the Node tools), runs their cells
//! natively in document order against one `:memory:` database ([`run`], with
//! the `engine` feature), and regenerates the cookbook index ([`index`]).
//! Results are rendered with `relate_formatter::recording`, the renderer
//! the browser uses through the WASM `renderResult`. `tests/recipes.rs` is
//! the CI gate; `docs/cookbook/README.md` is the authoring contract.

pub mod export;
pub mod frontmatter;
pub mod index;
pub mod recipe;
#[cfg(feature = "engine")]
pub mod run;

use std::path::{Path, PathBuf};

pub use recipe::{Cell, DataBlock, Recipe};

/// Largest data block allowed in a recipe.
pub const MAX_DATA_BYTES: usize = 512 * 1024;

/// Data blocks up to this size are parameters that sit beside their step and
/// are editable on the website; larger ones are datasets.
pub const INLINE_DATA_BYTES: usize = 2048;

/// Largest row count a single statement may return; recipes `LIMIT` beyond it.
pub const MAX_RESULT_ROWS: usize = 200;

/// Widest line a step's Cypher may have; wider lines hide behind a
/// horizontal scroll in the website's code panel.
pub const MAX_CODE_COLUMNS: usize = 88;

/// Most lines a recipe may run before its `## Data` section. The datasets in
/// that section are collapsed on GitHub and never rendered on the site, so
/// they do not count (ADR-0132).
pub const MAX_NARRATIVE_LINES: usize = 800;

/// The repository's `docs/cookbook` directory.
pub fn cookbook_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/cookbook")
}

/// The recipes under `root`, sorted by slug (only those named in `only`, when
/// given), and every problem loading them or, with `validate`, validating
/// them, each prefixed with the recipe's slug.
pub fn load_recipes(
    root: &Path,
    only: Option<&[String]>,
    validate: bool,
) -> std::io::Result<(Vec<Recipe>, Vec<String>)> {
    let mut recipes = Vec::new();
    let mut problems = Vec::new();
    for path in recipe_files(root)? {
        let slug = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if only.is_some_and(|only| !only.contains(&slug)) {
            continue;
        }
        match Recipe::load(&path) {
            Ok(recipe) => {
                if validate {
                    problems.extend(
                        recipe
                            .validate()
                            .into_iter()
                            .map(|e| format!("[{slug}] {e}")),
                    );
                }
                recipes.push(recipe);
            }
            Err(e) => problems.push(format!("[{slug}] {e}")),
        }
    }
    Ok((recipes, problems))
}

/// Every recipe file (`*.md` other than `README.md`), sorted by slug.
pub fn recipe_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        let is_recipe = path.is_file()
            && path.extension().is_some_and(|e| e == "md")
            && path.file_name().is_some_and(|n| n != "README.md");
        if is_recipe {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}
