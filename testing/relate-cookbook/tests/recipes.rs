//! The cookbook gate (ADR-0130, ADR-0132): every recipe under
//! `docs/cookbook/` runs natively and must reproduce the `table` / `gram`
//! result block recorded after each cell; the index in
//! `docs/cookbook/README.md` must be current.
//!
//! Environment:
//! - `COOKBOOK_UPDATE=1` rewrites the result blocks and the index instead of
//!   comparing them.
//! - `COOKBOOK_ONLY=<slug>[,<slug>…]` runs only the named recipes and skips
//!   the index.

use std::collections::BTreeMap;
use std::path::Path;

use relate_cookbook::{
    cookbook_dir, index, load_recipes,
    run::{check_run, run_recipe},
    Recipe,
};

fn write_if_changed(path: &Path, text: &str) {
    if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
        std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        eprintln!("updated {}", path.display());
    }
}

#[test]
fn recipes_reproduce_their_recorded_results() {
    let update = std::env::var("COOKBOOK_UPDATE").is_ok_and(|v| v == "1");
    let only: Option<Vec<String>> = std::env::var("COOKBOOK_ONLY")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let root = cookbook_dir();
    let (loaded, mut failures) =
        load_recipes(&root, only.as_deref(), true).expect("read docs/cookbook");
    let mut recipes: Vec<Recipe> = Vec::new();

    for recipe in loaded {
        let slug = recipe.slug.clone();
        let mut fail = |msg: String| failures.push(format!("[{slug}] {msg}"));
        let run = match run_recipe(&recipe) {
            Ok(r) => r,
            Err(e) => {
                fail(e);
                recipes.push(recipe);
                continue;
            }
        };
        let problems = check_run(&recipe, &run);
        let recordings: BTreeMap<String, _> =
            run.cells.into_iter().map(|c| (c.id, c.recording)).collect();

        if !problems.is_empty() {
            // Never record a run that went other than the recipe says.
            problems.into_iter().for_each(&mut fail);
        } else if update {
            write_if_changed(&recipe.path, &recipe.with_results(&recordings));
        } else {
            for cell in &recipe.cells {
                let actual = &recordings[&cell.id];
                match &cell.result {
                    None => fail(format!(
                        "cell `{}` has no result block; run with COOKBOOK_UPDATE=1",
                        cell.id
                    )),
                    Some(rec) if rec.kind != actual.kind() || rec.text != actual.text => {
                        fail(format!(
                            "cell `{}` result differs from its recording\n--- recorded ({})\n{}\n--- actual ({})\n{}",
                            cell.id,
                            rec.kind.info(),
                            rec.text,
                            actual.kind().info(),
                            actual.text
                        ))
                    }
                    _ => {}
                }
            }
        }
        recipes.push(recipe);
    }

    // The index covers every recipe, so a filtered run leaves it alone. It
    // reads only front matter, which an update does not change.
    if only.is_none() {
        let readme_path = root.join("README.md");
        let readme = std::fs::read_to_string(&readme_path).expect("read docs/cookbook/README.md");
        match index::splice(&readme, &index::render(&recipes)) {
            Err(e) => failures.push(e),
            Ok(fresh) if update => write_if_changed(&readme_path, &fresh),
            Ok(fresh) if fresh != readme => failures
                .push("docs/cookbook/README.md index is stale; run with COOKBOOK_UPDATE=1".into()),
            Ok(_) => {}
        }
    }

    assert!(
        failures.is_empty(),
        "{} cookbook problem(s):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
