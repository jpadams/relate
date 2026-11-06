//! `skill` command tests (moved out of skill_cmd.rs to keep both
//! under the repo's 1k-line file budget).

use super::*;
use crate::args;

/// A pre-existing destination holding only `file` with `contents`, under a
/// fresh tempdir: the fixture the refusal/force and recognizable-bundle tests
/// share. Returns the tempdir (its path is the skills dir) and the
/// destination.
fn dest_with(file: &str, contents: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = install_dir_for(dir.path());
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join(file), contents).unwrap();
    (dir, dest)
}

#[test]
fn parse_skill_args_reads_the_grammar_in_any_order() {
    let parsed = parse_skill_args(&args(&["install", "--agent", "claude", "--force"])).unwrap();
    assert_eq!(
        parsed,
        SkillArgs {
            action: SkillAction::Mutate(MutateAction::Install),
            agent: Some("claude".to_string()),
            force: true,
        }
    );
    let parsed = parse_skill_args(&args(&["remove", "--force", "--agent", "codex"])).unwrap();
    assert_eq!(
        parsed,
        SkillArgs {
            action: SkillAction::Mutate(MutateAction::Remove),
            agent: Some("codex".to_string()),
            force: true,
        }
    );
    let parsed = parse_skill_args(&args(&["install"])).unwrap();
    assert_eq!(
        parsed,
        SkillArgs {
            action: SkillAction::Mutate(MutateAction::Install),
            agent: None,
            force: false,
        }
    );
}

#[test]
fn parse_skill_args_usage_errors_are_pure_strings() {
    assert_eq!(
        parse_skill_args(&args(&[])).unwrap_err().to_string(),
        "skill requires an action (install, remove, list or check)"
    );
    assert_eq!(
        parse_skill_args(&args(&["bogus"])).unwrap_err().to_string(),
        "unexpected argument to skill: bogus"
    );
    // list/check are the read-only actions (ADR-0242); --force is only
    // for install/remove.
    for action in ["list", "check"] {
        let parsed = parse_skill_args(&args(&[action])).unwrap();
        assert_eq!(parsed.action.as_str(), action);
        assert_eq!(
            parse_skill_args(&args(&[action, "--force"]))
                .unwrap_err()
                .to_string(),
            "unexpected argument to skill: --force"
        );
    }
    assert_eq!(
        parse_skill_args(&args(&["install", "--bogus"]))
            .unwrap_err()
            .to_string(),
        "unexpected argument to skill: --bogus"
    );
    assert_eq!(
        parse_skill_args(&args(&["install", "--agent"]))
            .unwrap_err()
            .to_string(),
        "--agent requires a value (name of an AI agent)"
    );
}

#[test]
fn inject_version_replaces_placeholder() {
    let sample = "\
---
name: relate
description: something
version: {{VERSION}}
---
# body
";
    let out = String::from_utf8(inject_version(sample, "1.2.3")).unwrap();
    assert!(out.contains("\nversion: 1.2.3\n"), "got: {out}");
    assert!(!out.contains("{{VERSION}}"), "got: {out}");
}

#[test]
fn inject_version_inserts_when_absent() {
    let sample = "\
---
name: relate
description: something
---
# body
";
    let out = String::from_utf8(inject_version(sample, "0.3.0-alpha")).unwrap();
    assert!(out.contains("\nversion: 0.3.0-alpha\n"), "got: {out}");
    // The closing fence must still be the last frontmatter line.
    assert!(out.starts_with("---\nname: relate\n"));
    assert!(out.contains("description: something\nversion: 0.3.0-alpha\n---"));
}

#[test]
fn inject_version_no_frontmatter_returns_unchanged() {
    let sample = "# just a title\n\nno frontmatter";
    let out = String::from_utf8(inject_version(sample, "1.0.0")).unwrap();
    assert_eq!(out, sample);
}

#[test]
fn inject_version_handles_crlf_line_endings() {
    // Windows CI may bake CRLF into the embedded bundle via autocrlf.
    let sample = "---\r\nname: relate\r\ndescription: something\r\nversion: {{VERSION}}\r\n---\r\n# body\r\n";
    let out = String::from_utf8(inject_version(sample, "0.3.0-alpha")).unwrap();
    assert!(
        out.contains("version: 0.3.0-alpha\r\n"),
        "version not stamped under CRLF: got:\n{out}"
    );
    assert!(!out.contains("{{VERSION}}"), "placeholder leaked: {out}");
    // The closing fence must survive intact.
    assert!(out.contains("\r\n---\r\n"));
}

#[test]
fn skill_error_unknown_agent_lists_valid_agents() {
    let e = SkillError::UnknownAgent("bogus".to_string());
    let msg = e.stderr_message();
    assert!(msg.contains("unknown agent: 'bogus'"));
    assert!(msg.contains("valid agents:"));
    assert!(msg.contains("claude-code"));
}

#[test]
fn skill_error_no_agents_lists_valid_agents() {
    let msg = SkillError::NoAgentsDetected.stderr_message();
    assert!(msg.contains("no supported agents detected"));
    assert!(msg.contains("valid agents:"));
    assert!(msg.contains("claude-code"));
}

#[test]
fn install_copies_bundle_and_stamps_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    install_bundle(skills_dir, false).expect("install");
    let dest = install_dir_for(skills_dir);

    // Iterate the canonical bundle list: the drift-guard test below pins
    // BUNDLE_FILES to the on-disk tree, so this assertion tracks it by
    // construction instead of keeping a third hand-maintained copy.
    for (rel_path, _) in BUNDLE_FILES {
        assert!(dest.join(rel_path).is_file(), "{rel_path} missing");
    }
    assert!(
        dest.join(MARKER_FILE).is_file(),
        "ownership marker {MARKER_FILE} missing"
    );
    let skill_md = std::fs::read_to_string(dest.join("SKILL.md")).unwrap();
    assert!(
        skill_md.contains(&format!("version: {VERSION}")),
        "version not stamped: {}",
        skill_md
            .lines()
            .find(|l| l.starts_with("version:"))
            .unwrap_or("")
    );
    assert!(
        !skill_md.contains("{{VERSION}}"),
        "placeholder not replaced"
    );
}

#[test]
fn every_bundle_source_file_is_embedded() {
    // Drift guard: BUNDLE_FILES is hand-maintained, so a bundle file
    // that exists in the repo but is not listed there silently never
    // ships with `relate skill install`. Walk the source tree and
    // require an exact match with the embedded keys.
    let bundle_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/skill/bundle");
    // The same walk `bundle_drift` uses: one traversal, one symlink policy.
    let mut on_disk = walk_rel_file_paths(&bundle_dir).expect("read bundle dir");
    on_disk.sort();
    let mut embedded: Vec<String> = BUNDLE_FILES
        .iter()
        .map(|(rel_path, _)| rel_path.to_string())
        .collect();
    embedded.sort();
    assert_eq!(
        on_disk, embedded,
        "skill/bundle/ files and BUNDLE_FILES keys differ; list every \
             bundle file in BUNDLE_FILES in skill_cmd.rs"
    );
}

#[test]
fn remove_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    install_bundle(skills_dir, false).expect("install");
    let dest = install_dir_for(skills_dir);
    remove_bundle(skills_dir, false).expect("remove");
    assert!(!dest.exists(), "dest not removed");
    // Second remove on a missing dir is a no-op, not an error.
    remove_bundle(skills_dir, false).expect("idempotent remove");
}

#[test]
fn install_cleans_stale_references() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    let dest = install_dir_for(skills_dir);
    // A first (marked) install, then a stale file added, then reinstall:
    // the marker proves ownership so the clean rewrite is allowed.
    install_bundle(skills_dir, false).expect("install");
    std::fs::write(dest.join("references/stale.md"), "stale").unwrap();

    install_bundle(skills_dir, false).expect("reinstall");
    assert!(
        !dest.join("references/stale.md").exists(),
        "stale reference not cleaned"
    );
    assert!(dest.join("SKILL.md").is_file());
}

/// The replace is atomic: when the staged bundle cannot land, the
/// previous install is restored whole instead of being left removed — the
/// remove-then-rename shape this replaced would have deleted it before the
/// failing rename.
#[test]
fn a_failed_landing_restores_the_previous_bundle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    install_bundle(skills_dir, false).expect("install");
    let dest = install_dir_for(skills_dir);
    // A file inside the old bundle proves the restored copy is the previous
    // install, not a fresh one.
    std::fs::write(dest.join("previous-install.txt"), "kept").unwrap();

    // A staging path that does not exist makes the landing rename fail
    // after the previous install was parked aside.
    let missing_staging = crate::staging::staging_dir(skills_dir);
    let err = land_replacing(&dest, &missing_staging, skills_dir).unwrap_err();
    assert!(
        err.stderr_message().contains("renaming"),
        "{}",
        err.stderr_message()
    );
    assert!(
        dest.join("previous-install.txt").is_file(),
        "the previous install must be restored, not dropped"
    );
    assert!(dest.join(MARKER_FILE).is_file());
    assert!(dest.join("SKILL.md").is_file());
    // No parked-aside or staging residue was left behind.
    let residue: Vec<_> = std::fs::read_dir(skills_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| crate::staging::is_staging_name(&entry.file_name()))
        .collect();
    assert!(residue.is_empty(), "installer residue: {residue:?}");
}

#[test]
fn install_refuses_unmarked_dir_without_force() {
    let (dir, dest) = dest_with("sentinel.txt", "mine");

    let err = install_bundle(dir.path(), false).unwrap_err();
    assert!(matches!(err, SkillError::NotOwned(_)));
    assert!(
        err.stderr_message().contains(MARKER_FILE),
        "message must name the marker: {}",
        err.stderr_message()
    );
    assert!(
        dest.join("sentinel.txt").is_file(),
        "unmarked content must be untouched"
    );
    assert!(!dest.join("SKILL.md").exists());
}

#[test]
fn install_force_overwrites_unmarked_dir_and_marks_it() {
    let (dir, dest) = dest_with("sentinel.txt", "mine");

    install_bundle(dir.path(), true).expect("forced install");
    assert!(dest.join(MARKER_FILE).is_file());
    assert!(dest.join("SKILL.md").is_file());
    assert!(
        !dest.join("sentinel.txt").exists(),
        "forced install must clean the old dir"
    );
}

#[test]
fn install_adopts_recognizable_bundle_without_force() {
    // ADR-0262: an unmarked but recognizable relate bundle (name +
    // version in SKILL.md frontmatter) is replaced without `--force`, so
    // the install process re-validates ownership at the moment of use.
    let (dir, dest) = dest_with(
        "SKILL.md",
        "---\nname: relate\nversion: 0.7.0\n---\nold bundle\n",
    );

    install_bundle(dir.path(), false).expect("adopting install");
    assert!(dest.join(MARKER_FILE).is_file(), "marker not stamped");
    let skill_md = std::fs::read_to_string(dest.join("SKILL.md")).unwrap();
    assert!(
        skill_md.contains(&format!("version: {VERSION}")),
        "adopted bundle not restamped: {skill_md}"
    );
}

#[test]
fn install_adopts_version_placeholder_bundle_without_force() {
    let (dir, dest) = dest_with(
        "SKILL.md",
        "---\nname: relate\nversion: {{VERSION}}\n---\nold bundle\n",
    );

    install_bundle(dir.path(), false).expect("adopting install");
    assert!(dest.join(MARKER_FILE).is_file());
}

#[test]
fn install_refuses_name_only_bundle_without_force() {
    // LABS-84 / ADR-0262: `name: relate` alone is not recognizable; a
    // hand-authored directory sharing the name must never be replaced.
    let (dir, dest) = dest_with(
        "SKILL.md",
        "---\nname: relate\ndescription: mine\n---\nhand-written\n",
    );

    let err = install_bundle(dir.path(), false).unwrap_err();
    assert!(matches!(err, SkillError::NotOwned(_)), "got: {err:?}");
    assert!(
        dest.join("SKILL.md").is_file(),
        "name-only content must be untouched"
    );
}

#[test]
fn remove_refuses_recognizable_unmarked_bundle_without_force() {
    // ADR-0262: remove keeps the strict marker requirement; an unmarked
    // (even recognizable) directory must not be deleted.
    let (dir, dest) = dest_with(
        "SKILL.md",
        "---\nname: relate\nversion: 0.7.0\n---\nold bundle\n",
    );

    let err = remove_bundle(dir.path(), false).unwrap_err();
    assert!(matches!(err, SkillError::NotOwned(_)), "got: {err:?}");
    assert!(dest.exists(), "unmarked bundle must survive refused remove");
}

#[test]
fn remove_refuses_unmarked_dir_without_force() {
    let (dir, dest) = dest_with("sentinel.txt", "mine");

    let err = remove_bundle(dir.path(), false).unwrap_err();
    assert!(matches!(err, SkillError::NotOwned(_)));
    assert!(
        dest.join("sentinel.txt").is_file(),
        "unmarked content must survive refused remove"
    );
}

#[test]
fn remove_force_deletes_unmarked_dir() {
    let (dir, dest) = dest_with("sentinel.txt", "mine");

    remove_bundle(dir.path(), true).expect("forced remove");
    assert!(!dest.exists(), "forced remove must delete the dir");
}

#[test]
fn marked_reinstall_succeeds_without_force() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    install_bundle(skills_dir, false).expect("first install");
    // The marker from the first install permits a second, unforced install.
    install_bundle(skills_dir, false).expect("marked reinstall");
}

#[cfg(unix)]
#[test]
fn install_refuses_symlinked_marker_without_force() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    let dest = install_dir_for(skills_dir);
    std::fs::create_dir_all(&dest).unwrap();
    // A marker symlink pointing at a real regular file must not be trusted:
    // the check must not follow the link.
    let real_marker = dir.path().join("real-marker");
    std::fs::write(&real_marker, b"").unwrap();
    std::os::unix::fs::symlink(&real_marker, dest.join(MARKER_FILE)).unwrap();

    let err = install_bundle(skills_dir, false).unwrap_err();
    assert!(matches!(err, SkillError::NotOwned(_)), "got: {err:?}");
    assert!(
        !dest.join("SKILL.md").exists(),
        "symlinked marker must not permit install"
    );
}

#[cfg(unix)]
#[test]
fn install_refuses_symlinked_dest_without_force() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    let dest = install_dir_for(skills_dir);
    // A real, marked target elsewhere; `dest` is a symlink to it.
    let target = dir.path().join("elsewhere");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join(MARKER_FILE), b"").unwrap();
    std::os::unix::fs::symlink(&target, &dest).unwrap();

    let err = install_bundle(skills_dir, false).unwrap_err();
    assert!(matches!(err, SkillError::NotOwned(_)), "got: {err:?}");
}

#[cfg(unix)]
#[test]
fn install_refuses_uninspectable_destination() {
    // A metadata error other than NotFound (here ENOTDIR: the skills dir is
    // a regular file) fails closed as NotOwned, never as a fresh install.
    // The ENOTDIR simulation is Unix-only: Windows returns NotFound for a
    // path below a regular file.
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path().join("not-a-dir");
    std::fs::write(&skills_dir, b"x").unwrap();

    let err = install_bundle(&skills_dir, false).unwrap_err();
    assert!(matches!(err, SkillError::NotOwned(_)), "got: {err:?}");
}

#[test]
fn install_sweeps_stale_staging_dirs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    // Simulate a crashed install: a leftover staging dir with content.
    let stale = skills_dir.join(".relate-stage-123-456");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("SKILL.md"), "half-written").unwrap();

    // Only residue old enough is reclaimed (a fresh sibling may be another
    // process's live install): age the leftover past the shared threshold.
    // A directory's mtime is settable through a directory handle on unix;
    // the age rule itself is pinned cross-platform in `staging.rs`.
    #[cfg(unix)]
    {
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60);
        std::fs::File::open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();
        install_bundle(skills_dir, false).expect("install");
        assert!(!stale.exists(), "stale staging dir not swept");
    }
    #[cfg(not(unix))]
    {
        // A fresh sibling is a live install's, so it survives.
        install_bundle(skills_dir, false).expect("install");
        assert!(stale.exists(), "a fresh staging dir is not residue");
    }
    assert!(
        install_dir_for(skills_dir).join("SKILL.md").is_file(),
        "install must still succeed"
    );
}

#[test]
fn install_leaves_no_staging_residue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills_dir = dir.path();
    install_bundle(skills_dir, false).expect("install");

    let residue: Vec<_> = std::fs::read_dir(skills_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".relate-stage-"))
        .collect();
    assert!(
        residue.is_empty(),
        "staging residue left behind: {residue:?}"
    );
}
