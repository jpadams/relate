use super::*;

// --- the `--from` flow (ADR-0431 §5) end to end ---

#[test]
fn from_dir_installs_the_accounted_set_with_the_marker_and_the_loud_line() {
    let world = World::new();
    let source = world.source_package("greet", "1.2.3");
    install_from(&world, "greet", &source).unwrap();
    let target = world.extensions_dir().join("greet");
    // The installed dir holds exactly the accounted set plus the marker.
    assert!(target.join("extension.json").is_file());
    assert!(target.join("run.sh").is_file());
    assert!(target.join(MARKER_FILE).is_file());
    let manifest: Manifest =
        serde_json::from_str(&std::fs::read_to_string(target.join("extension.json")).unwrap())
            .unwrap();
    let Manifest::Command(command) = &manifest;
    assert_eq!(command.version, "1.2.3");
    // The loud line (§6's shape, the local tier's source reference): the
    // version read back from what landed, the source as named.
    assert_eq!(
        loud_line(
            "greet",
            "1.2.3",
            &Provenance {
                tier: Tier::Local,
                source: source.display().to_string(),
                pin: None
            }
        ),
        format!(
            "installed local extension greet 1.2.3 from {}",
            source.display()
        )
    );
    // The source directory is left untouched (§5).
    assert!(source.join("extension.json").is_file());
    assert!(source.join("run.sh").is_file());
    // No staging residue: the install landed through the staged sibling.
    let residue: Vec<_> = std::fs::read_dir(world.extensions_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| crate::staging::is_staging_name(&entry.file_name()))
        .collect();
    assert!(residue.is_empty(), "staging residue: {residue:?}");
}

#[test]
fn from_dir_reinstalls_the_same_tier_cleanly_on_a_version_change() {
    let world = World::new();
    let v1 = world.source_package("greet", "1.0.0");
    install_from(&world, "greet", &v1).unwrap();
    // A version change over the same tier is the clean replace (§3).
    let v2 = world.source_package("greet", "2.0.0");
    install_from(&world, "greet", &v2).unwrap();
    let manifest: Manifest = serde_json::from_str(
        &std::fs::read_to_string(world.extensions_dir().join("greet").join("extension.json"))
            .unwrap(),
    )
    .unwrap();
    let Manifest::Command(command) = &manifest;
    assert_eq!(command.version, "2.0.0");
}

#[test]
fn from_dir_refuses_each_manifest_problem_with_its_class() {
    let world = World::new();
    // A malformed manifest: the manifest problem class (exit 2).
    let dir = world.tmp.path().join("broken");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("extension.json"), "{ nope").unwrap();
    std::fs::write(dir.join("run.sh"), "#!/bin/sh\n").unwrap();
    let err = refusal(install_from(&world, "greet", &dir));
    assert_eq!(err.exit_code(), 2, "{}", err.stderr_message());
    assert!(
        err.stderr_message().contains("invalid manifest"),
        "{}",
        err.stderr_message()
    );
    // A name mismatch: the request and the package's own name disagree.
    let source = world.source_package("greet", "1.0.0");
    let err = refusal(install_from(&world, "other", &source));
    assert_eq!(err.exit_code(), 1);
    assert_eq!(
        err.stderr_message(),
        "Error: extension install: the package's manifest names 'greet' but the install was for 'other'"
    );
    // An escaping executable: the registry's pinned load refusal (§4
    // step 5's same-texts rule), exit 2.
    let dir = world.tmp.path().join("escape");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("extension.json"),
        manifest_text("greet", "1.0.0").replace("\"run.sh\"", "\"../evil.sh\""),
    )
    .unwrap();
    std::fs::write(dir.join("run.sh"), "#!/bin/sh\n").unwrap();
    let err = refusal(install_from(&world, "greet", &dir));
    assert_eq!(err.exit_code(), 2);
    assert_eq!(
        err.stderr_message(),
        "Error: extension install: extension 'greet': executable path '../evil.sh' escapes the package directory"
    );
    // A protocol the host does not speak: the manifest problem class
    // (ADR-0431 §3), naming both numbers (§8).
    let dir = world.tmp.path().join("proto2");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("extension.json"),
        manifest_text("greet", "1.0.0")
            .replace("\"protocol\":\"2025-06-18\"", "\"protocol\":\"1999-01-01\""),
    )
    .unwrap();
    std::fs::write(dir.join("run.sh"), "#!/bin/sh\n").unwrap();
    let err = refusal(install_from(&world, "greet", &dir));
    assert_eq!(err.exit_code(), 2);
    assert_eq!(
        err.stderr_message(),
        "Error: extension install: package 'greet' 1.0.0 speaks protocol '1999-01-01', \
         but this host speaks '2025-06-18'"
    );
    // A missing manifest in the source directory.
    let dir = world.tmp.path().join("empty");
    std::fs::create_dir_all(&dir).unwrap();
    let err = refusal(install_from(&world, "greet", &dir));
    assert_eq!(err.exit_code(), 1);
    assert!(
        err.stderr_message().contains("reading") && err.stderr_message().contains("extension.json"),
        "{}",
        err.stderr_message()
    );
}

#[cfg(unix)]
#[test]
fn from_dir_refuses_a_symlinked_accounted_file() {
    // The closed contract's parity (§2: regular files only) — a symlinked
    // source never rides out-of-directory content into the XDG dir.
    let world = World::new();
    let dir = world.tmp.path().join("linked");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("extension.json"), manifest_text("greet", "1.0.0")).unwrap();
    // Pin the manifest's mode: the writability check judges the source's
    // write bits, and this test is about the symlinked executable.
    std::fs::set_permissions(
        dir.join("extension.json"),
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .unwrap();
    std::fs::write(world.tmp.path().join("outside.sh"), "stolen\n").unwrap();
    std::os::unix::fs::symlink(world.tmp.path().join("outside.sh"), dir.join("run.sh")).unwrap();
    let err = refusal(install_from(&world, "greet", &dir));
    assert_eq!(err.exit_code(), 1);
    assert!(
        err.stderr_message().contains("is not a regular file"),
        "{}",
        err.stderr_message()
    );
}

/// The intermediate-directory symlink: `bin` pointing outside the
/// package directory. The executable's own name is a plain relative
/// path and the file it reaches is a regular file — both earlier checks
/// pass — but the path it RESOLVES to is outside the source directory,
/// and canonicalize + containment is what refuses it (a lexical
/// containment alone never would).
#[cfg(unix)]
#[test]
fn from_dir_refuses_an_executable_reached_through_a_directory_symlink() {
    let world = World::new();
    let dir = world.tmp.path().join("pkg");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("extension.json"),
        manifest_text("greet", "1.0.0").replace("\"run.sh\"", "\"bin/greet.sh\""),
    )
    .unwrap();
    // Pin the manifest's mode: the writability check judges the source's
    // write bits, and this test is about the directory symlink.
    std::fs::set_permissions(
        dir.join("extension.json"),
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .unwrap();
    let outside = world.tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("greet.sh"), "#!/bin/sh\n").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("bin")).unwrap();
    let err = refusal(install_from(&world, "greet", &dir));
    assert_eq!(err.exit_code(), 1, "{}", err.stderr_message());
    assert!(
        err.stderr_message()
            .contains("resolves outside the package directory"),
        "{}",
        err.stderr_message()
    );
    // Nothing landed from the refused install.
    assert!(!world.extensions_dir().join("greet").exists());
}

/// The `--from` copy lands the source executable's mode through the
/// unpacker's one masking policy (`apply_mode`, ADR-0431 §2): a setgid
/// source lands masked to its rwx bits, and a plain 0o755 source lands
/// unchanged.
#[cfg(unix)]
#[test]
fn from_dir_masks_the_source_mode_like_the_unpacker() {
    use std::os::unix::fs::PermissionsExt;
    let mode_of = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o7777;

    let world = World::new();
    let source = world.source_package("greet", "1.0.0");
    std::fs::set_permissions(
        source.join("run.sh"),
        std::fs::Permissions::from_mode(0o2755),
    )
    .unwrap();
    install_from(&world, "greet", &source).unwrap();
    assert_eq!(
        mode_of(&world.extensions_dir().join("greet").join("run.sh")),
        0o755,
        "the setgid bit rode the copy instead of being masked"
    );

    let world = World::new();
    let source = world.source_package("greet", "1.0.0");
    std::fs::set_permissions(
        source.join("run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    install_from(&world, "greet", &source).unwrap();
    assert_eq!(
        mode_of(&world.extensions_dir().join("greet").join("run.sh")),
        0o755,
        "a normal 0o755 source must land unchanged"
    );
}

/// A group- or world-writable source file refuses before the copy: another
/// account could swap it between the checks and the landing. The refusal
/// names the file and the offending bits, and nothing lands.
#[cfg(unix)]
#[test]
fn from_dir_refuses_a_group_or_world_writable_source() {
    use std::os::unix::fs::PermissionsExt;
    for mode in [0o775u32, 0o757] {
        let world = World::new();
        let source = world.source_package("greet", "1.0.0");
        let executable = source.join("run.sh");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(mode)).unwrap();
        let err = refusal(install_from(&world, "greet", &source));
        assert_eq!(err.exit_code(), 1, "{}", err.stderr_message());
        assert_eq!(
            err.stderr_message(),
            format!(
                "Error: extension install: {} is writable by group or others (mode {:03o}, \
                 offending bits {:03o}); --from refuses a source file other accounts can \
                 change — clear the group and other write bits",
                executable.display(),
                mode,
                mode & 0o022
            )
        );
        assert!(!world.extensions_dir().join("greet").exists());
    }
}

#[test]
fn a_failed_install_leaves_no_residue_and_no_target() {
    let world = World::new();
    let dir = world.tmp.path().join("broken");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("extension.json"), "{ nope").unwrap();
    std::fs::write(dir.join("run.sh"), "#!/bin/sh\n").unwrap();
    assert!(install_from(&world, "greet", &dir).is_err());
    assert!(!world.extensions_dir().join("greet").exists());
    let residue: Vec<_> = std::fs::read_dir(world.extensions_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| crate::staging::is_staging_name(&entry.file_name()))
        .collect();
    assert!(residue.is_empty(), "staging residue: {residue:?}");
}
