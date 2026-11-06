//! The plan-level tests: the ADR-0377 §5 path rule, the per-commit path
//! dedupe, the pair counting (ADR-0376, ADR-0394), and the phases' row
//! shapes. The `Arc`-handle sharing the CLI importer's rows carried
//! (ADR-0377) is a plan-internal property now — the wire's rows are
//! JSON, built fresh per chunk — so what a test can pin is each phase's
//! row content, derived from the one plan.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;

use super::*;
use crate::git_log::GitCommit;
use crate::{DEFAULT_MAX_CHANGESET, MAX_CO_CHANGED_PAIRS};

#[test]
fn mapped_path_pushes_every_component_onto_dir() {
    let dir = Path::new("/tmp/a root");
    assert_eq!(
        mapped_path(dir, &"a".repeat(40), "src/with space/å.txt").unwrap(),
        dir.join("src").join("with space").join("å.txt")
    );
    assert!(mapped_path(dir, &"a".repeat(40), "").is_err());
    assert!(mapped_path(dir, &"a".repeat(40), "src/").is_err());
}

#[test]
fn mapped_path_rejects_non_normal_components_naming_the_commit_record() {
    let sha = "a".repeat(40);
    let dir = Path::new("/repo");
    for (git_path, component) in [
        ("..", ".."),
        ("a/../..", ".."),
        (".", "."),
        ("/etc/passwd", ""),
        ("C:/Windows", "C:"),
    ] {
        let err = mapped_path(dir, &sha, git_path).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "malformed git path {git_path:?} in commit {sha}: \
                 component {component:?} is not allowed"
            )
        );
    }

    let err = mapped_path(dir, &sha, "").unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("malformed git path \"\" in commit {sha}: path is empty")
    );
}

/// The `statement` phase's rows, collected by its write statement so no
/// test re-splits the six-phase order `Plan::phases` owns (ADR-0392).
fn phase_rows(plan: &Plan, statement: &str) -> Vec<Value> {
    plan.phases()
        .into_iter()
        .find(|(candidate, _)| *candidate == statement)
        .unwrap_or_else(|| panic!("the plan has no {statement} write phase"))
        .1
        .collect()
}

/// One two-commit history over two shared paths: the fixture the row-shape
/// and person-name tests build on.
fn two_commit_history() -> Vec<GitCommit> {
    vec![
        GitCommit {
            sha: "a".repeat(40),
            author_name: "Ada".to_string(),
            author_email: "ada@example.com".to_string(),
            authored_at: "2026-01-01T00:00:00Z".to_string(),
            subject: "one".to_string(),
            paths: vec!["a.txt".to_string(), "b.txt".to_string()],
        },
        GitCommit {
            sha: "b".repeat(40),
            author_name: "Grace".to_string(),
            author_email: "grace@example.com".to_string(),
            authored_at: "2026-01-02T00:00:00Z".to_string(),
            subject: "two".to_string(),
            paths: vec!["a.txt".to_string()],
        },
    ]
}

#[test]
fn the_phases_carry_the_planned_rows_in_adr_0376s_order() {
    let plan = build(
        Path::new("/repo"),
        two_commit_history(),
        DEFAULT_MAX_CHANGESET,
        MAX_CO_CHANGED_PAIRS,
    )
    .unwrap();
    assert_eq!(
        plan.stats(),
        Stats {
            commits: 2,
            people: 2,
            files: 2,
            touched: 3,
            co_changed: 1,
        }
    );

    // The phase order is ADR-0376's: persons, commits, files, then the
    // three edge phases.
    let order: Vec<&str> = plan
        .phases()
        .iter()
        .map(|(statement, _)| *statement)
        .collect();
    assert_eq!(
        order,
        [
            WRITE_PERSONS,
            WRITE_COMMITS,
            WRITE_FILES,
            WRITE_AUTHORED,
            WRITE_TOUCHED,
            WRITE_CO_CHANGED,
        ]
    );

    let persons = phase_rows(&plan, WRITE_PERSONS);
    let commits = phase_rows(&plan, WRITE_COMMITS);
    let files = phase_rows(&plan, WRITE_FILES);
    let touched = phase_rows(&plan, WRITE_TOUCHED);
    let co_changed = phase_rows(&plan, WRITE_CO_CHANGED);

    // The File row's name is derived from the stored path once; the
    // person row carries the newest commit's name.
    assert_eq!(files.len(), 2);
    let file_a = files
        .iter()
        .find(|row| row["path"] == "/repo/a.txt")
        .expect("a.txt has a File row");
    assert_eq!(file_a["name"], "a.txt");
    assert_eq!(
        persons
            .iter()
            .find(|row| row["email"] == "ada@example.com")
            .expect("Ada has a person row")["name"],
        "Ada"
    );

    // The commit row's files_changed is its path count, written from the
    // plan — never a second field.
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["files_changed"], 2);
    assert_eq!(commits[0]["authored_at"], "2026-01-01T00:00:00Z");

    // TOUCHED rows: commit A's a.txt and b.txt, commit B's a.txt.
    assert_eq!(touched.len(), 3);
    assert_eq!(touched[0]["sha"], "a".repeat(40));
    assert_eq!(touched[0]["path"], "/repo/a.txt");
    assert_eq!(touched[1]["path"], "/repo/b.txt");
    assert_eq!(touched[2]["sha"], "b".repeat(40));

    // The CO_CHANGED pair runs from the smaller stored path to the larger.
    assert_eq!(co_changed.len(), 1);
    assert_eq!(co_changed[0]["a"], "/repo/a.txt");
    assert_eq!(co_changed[0]["b"], "/repo/b.txt");
    assert_eq!(co_changed[0]["count"], 1);
}

#[test]
fn co_changed_pair_counts_skip_oversized_changesets() {
    let changesets = [
        vec!["a", "b"],
        vec!["a", "b", "c"],
        vec!["a", "b", "c", "d"],
    ];
    let mut counts = BTreeMap::new();
    for paths in changesets {
        let changeset: BTreeSet<Arc<str>> = paths.iter().map(|path| Arc::from(*path)).collect();
        count_co_changes(&changeset, 3, MAX_CO_CHANGED_PAIRS, &mut counts).unwrap();
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            ((Arc::from("a"), Arc::from("b")), 2),
            ((Arc::from("a"), Arc::from("c")), 1),
            ((Arc::from("b"), Arc::from("c")), 1),
        ])
    );
    assert!(
        counts.keys().all(|(a, b)| a < b),
        "every edge must run from the smaller path: {counts:?}"
    );
}

#[test]
fn a_commit_listing_one_path_twice_plans_one_touched_row() {
    // `git log --name-only` can list one path twice for a commit; the
    // plan's touched rows must stay distinct (ADR-0394) — a duplicate
    // would land a parallel edge the `MERGE` statements collapse anyway.
    let history = vec![GitCommit {
        sha: "a".repeat(40),
        author_name: "Ada".to_string(),
        author_email: "ada@example.com".to_string(),
        authored_at: "2026-01-01T00:00:00Z".to_string(),
        subject: "touch one path twice".to_string(),
        paths: vec![
            "a.txt".to_string(),
            "b.txt".to_string(),
            "a.txt".to_string(),
        ],
    }];
    let plan = build(
        Path::new("/repo"),
        history,
        DEFAULT_MAX_CHANGESET,
        MAX_CO_CHANGED_PAIRS,
    )
    .unwrap();
    assert_eq!(plan.stats().touched, 2);
    assert_eq!(phase_rows(&plan, WRITE_TOUCHED).len(), 2);
}

#[test]
fn too_many_pairs_fails_while_building_the_plan() {
    // The pair cap (ADR-0377 §2) fires while the history is folded, before
    // the child emits its first record — an import that would refuse has
    // emitted nothing.
    let history = vec![GitCommit {
        sha: "a".repeat(40),
        author_name: "Ada".to_string(),
        author_email: "ada@example.com".to_string(),
        authored_at: "2026-01-01T00:00:00Z".to_string(),
        subject: "a three-file change".to_string(),
        paths: vec![
            "a.txt".to_string(),
            "b.txt".to_string(),
            "c.txt".to_string(),
        ],
    }];
    // The fold's refusal, by variant — the plan carries no `Debug` and
    // the error is the payload under test, so `match` it rather than
    // unwrap.
    let err = match build(Path::new("/repo"), history, DEFAULT_MAX_CHANGESET, 2) {
        Ok(_) => panic!("the pair cap must refuse the three-file change"),
        Err(err) => err,
    };
    assert_eq!(
        err.to_string(),
        "import would write more than 2 CO_CHANGED pairs; \
         lower --max-changeset or pass --max-commits to read fewer commits"
    );
}
