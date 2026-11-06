//! `extension init`'s grammar and scaffold tests: the closed `--lang`
//! vocabulary, the package-name rule (and the reserved-word collision), the
//! one-positional shape, and the files a scaffold leaves on disk. The end to
//! end proof — scaffold, `install --from`, `run` — is
//! `tests/e2e_extension_init.rs`.

use super::*;

fn args(xs: &[&str]) -> Vec<String> {
    crate::args(xs)
}

fn err_of(xs: &[&str]) -> String {
    parse_init_args(&args(xs)).unwrap_err().to_string()
}

/// The default language is `py`; `--lang` picks the template and takes no
/// second value.
#[test]
fn the_grammar_reads_the_name_and_the_language() {
    let parsed = parse_init_args(&args(&["my-ext"])).unwrap();
    assert_eq!(parsed.name, "my-ext");
    assert_eq!(parsed.language, templates::Language::Py);

    let parsed = parse_init_args(&args(&["my-ext", "--lang", "sh"])).unwrap();
    assert_eq!(parsed.language, templates::Language::Sh);
    // Any order: the flag before the positional.
    let parsed = parse_init_args(&args(&["--lang", "rust", "my-ext"])).unwrap();
    assert_eq!(parsed.name, "my-ext");
    assert_eq!(parsed.language, templates::Language::Rust);
}

#[test]
fn the_name_and_language_are_usage_errors_when_missing_or_unknown() {
    assert_eq!(err_of(&[]), "extension init requires a package name");
    assert_eq!(
        err_of(&["my-ext", "--lang", "python"]),
        "extension init: unknown --lang value 'python' (expected sh, py, ts or rust)"
    );
    assert_eq!(
        err_of(&["my-ext", "--lang"]),
        "--lang requires a value (sh, py, ts or rust)"
    );
    assert_eq!(
        err_of(&["a", "b"]),
        "unexpected argument to extension init: b"
    );
    assert_eq!(
        err_of(&["my-ext", "--bogus"]),
        "unexpected argument to extension init: --bogus"
    );
    assert_eq!(
        err_of(&["my-ext", "--lang", "sh", "--lang", "py"]),
        "extension init --lang may be given only once"
    );
}

/// A package name is the installer's own rule — one directory segment — and
/// never a reserved word: the collision the registry would refuse at load
/// (ADR-0429 §6) is refused here, before anything is written, with the
/// registry's typed text.
#[test]
fn the_name_rule_is_the_installers_and_refuses_reserved_words() {
    for bad in ["", "a/b", "a\\b", ".", ".."] {
        let err = err_of(&[bad]);
        assert!(
            err.contains("is not a valid package name"),
            "{bad:?} must be refused: {err}"
        );
    }
    // Dot-separated and spaced names are directories the installer accepts,
    // so the scaffold accepts them too — one rule, not two.
    for ok in ["a.b", "a b", "fs_to_graph-2"] {
        parse_init_args(&args(&[ok])).expect("the installer's rule accepts it");
    }
    // A flag-shaped word never reaches the name position.
    assert_eq!(err_of(&["-x"]), "unexpected argument to extension init: -x");
    assert_eq!(
        err_of(&["run"]),
        "extension name 'run' collides with the reserved command word 'run'"
    );
    assert_eq!(
        err_of(&["extension"]),
        "extension name 'extension' collides with the reserved command word 'extension'"
    );
}

/// The predicate itself is the shared one: a NUL byte is refused although no
/// argv can carry it.
#[test]
fn the_package_name_predicate_refuses_a_nul() {
    assert!(!crate::extensions::is_package_name("a\0b"));
    assert!(crate::extensions::is_package_name("a.b"));
}

/// The scaffold writes the whole package and nothing else, the executable
/// lands executable, and a second run refuses the existing directory.
#[test]
fn the_scaffold_writes_one_package_and_refuses_to_clobber_it() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = InitSpec {
        name: "my-ext".to_string(),
        language: templates::Language::Py,
    };
    let (dir, _files) = scaffold(&spec, tmp.path()).expect("the scaffold writes");
    assert_eq!(dir, tmp.path().join("my-ext"));

    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["README.md", "extension.json", "my-ext.py"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("my-ext.py"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the executable lands executable");
    }

    // A second init into the same directory is the refusal, and the message
    // names the directory it found.
    let err = scaffold(&spec, tmp.path()).unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert!(err.contains("my-ext"), "{err}");
}

/// The Rust package nests its crate under `src/` and leaves the executable to
/// the build — the one language whose executable is not a generated file.
#[test]
fn the_rust_scaffold_writes_a_crate_beside_the_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = InitSpec {
        name: "my-ext".to_string(),
        language: templates::Language::Rust,
    };
    let (dir, _files) = scaffold(&spec, tmp.path()).expect("the scaffold writes");
    for path in ["extension.json", "Cargo.toml", "src/main.rs", "README.md"] {
        assert!(dir.join(path).is_file(), "{path} is missing");
    }
    // Nothing claims the exec bit: the built binary is the executable.
    assert!(!dir.join("my-ext").exists());
}

/// The block `init` prints names every generated file through its role —
/// the quickstart the author follows out of the terminal — and the scaffold
/// writes exactly the files the block lists.
#[test]
fn the_created_block_names_the_files_through_their_roles() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = InitSpec {
        name: "my-ext".to_string(),
        language: templates::Language::Rust,
    };
    // One render: the block prints the files the scaffold wrote.
    let (dir, files) = scaffold(&spec, tmp.path()).unwrap();
    for file in &files {
        assert!(!file.role.description().is_empty(), "{}", file.path);
        assert!(dir.join(&file.path).is_file(), "{} written", file.path);
    }
    let roles: Vec<templates::Role> = files.iter().map(|file| file.role).collect();
    // The Rust package's roles, from the one role vocabulary: every role but
    // the glue executable (its executable is what the crate builds).
    let expected: Vec<templates::Role> = templates::Role::ALL
        .into_iter()
        .filter(|role| *role != templates::Role::GlueExecutable)
        .collect();
    assert_eq!(roles, expected);
}
