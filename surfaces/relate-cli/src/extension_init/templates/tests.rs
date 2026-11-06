//! The templates' cross-language contract: every language scaffolds the
//! same package shape (a decodable manifest, one executable, a quickstart
//! README), every glue file names the transport contract, and no placeholder
//! survives rendering.
//!
//! The language-specific assertions (a path, a shebang, a walk signature, the
//! no-import rule) live in each template module; everything a template could
//! get wrong on its own is pinned here, once, by role.

use super::*;
use crate::extensions::manifest::{FlagSpec, Manifest};

/// Whether a file of `role` is generated glue — the files that name the
/// transport contract, carry the walk and are what an author edits.
fn is_glue(role: Role) -> bool {
    matches!(role, Role::GlueExecutable | Role::GlueSource)
}

/// The rendered package for `language`, checked for leftovers: no placeholder
/// may survive [`render`] in any file.
fn package(language: Language, name: &str) -> Vec<PackageFile> {
    let files = language.package(name);
    for file in &files {
        for placeholder in [
            NAME_PLACEHOLDER,
            GLUE_SOURCE_PLACEHOLDER,
            PROTOCOL_PLACEHOLDER,
        ] {
            assert!(
                !file.contents.contains(placeholder),
                "{} left {placeholder:?} in {}",
                language.as_str(),
                file.path
            );
        }
    }
    files
}

/// The files of `role` in `files`.
fn with_role(files: &[PackageFile], role: Role) -> Vec<&PackageFile> {
    files.iter().filter(|file| file.role == role).collect()
}

/// Every language's manifest decodes as the command stratum, names the
/// package, speaks this host's MCP revision, and declares an executable the
/// package accounts for.
#[test]
fn every_language_scaffolds_a_decodable_command_manifest() {
    for language in Language::ALL {
        let files = package(language, "my-ext");
        let manifest_file = with_role(&files, Role::Manifest);
        assert_eq!(
            manifest_file.len(),
            1,
            "{}: one manifest",
            language.as_str()
        );
        let manifest: Manifest = serde_json::from_str(&manifest_file[0].contents)
            .unwrap_or_else(|err| panic!("{}: manifest does not decode: {err}", language.as_str()));
        let Manifest::Command(command) = &manifest;
        assert_eq!(command.name, "my-ext");
        assert_eq!(command.version, "0.1.0");
        assert_eq!(command.protocol, MCP_PROTOCOL_VERSION);
        // The executable the manifest names is the file the package ships as
        // its executable — or, for the crate languages, the binary the build
        // produces.
        let declared = command.executable.to_string_lossy().into_owned();
        match with_role(&files, Role::GlueExecutable).as_slice() {
            [executable] => assert_eq!(declared, executable.path),
            [] => assert_eq!(
                declared,
                "my-ext",
                "{}: the built binary",
                language.as_str()
            ),
            many => panic!(
                "{}: {} executable files, a package ships one",
                language.as_str(),
                many.len()
            ),
        }

        // The one flag every generated walk binds end to end: `--name`.
        assert_eq!(
            command.flags.len(),
            1,
            "{}: one sample flag",
            language.as_str()
        );
        let FlagSpec::String(flag) = &command.flags[0] else {
            panic!("{}: the sample flag is a string", language.as_str());
        };
        assert_eq!(flag.name, "name");
        assert_eq!(flag.default, "there");
    }
}

/// Every package carries exactly one README and at least one glue file, and
/// every glue file — the file an author edits — carries the walk's TODO.
#[test]
fn every_language_ships_one_readme_and_todo_marked_glue() {
    for language in Language::ALL {
        let files = package(language, "my-ext");
        assert_eq!(
            with_role(&files, Role::Readme).len(),
            1,
            "{}",
            language.as_str()
        );
        let glue = files
            .iter()
            .filter(|file| is_glue(file.role))
            .collect::<Vec<_>>();
        assert_eq!(glue.len(), 1, "{}: one glue file", language.as_str());
        assert!(
            glue[0].contents.contains("TODO"),
            "{}: {} has no TODO marker",
            language.as_str(),
            glue[0].path
        );
    }
}

/// The glue's header names the contract it copies — the whole point of the
/// generated glue: a contract change is a re-scaffold, not a version bump.
/// The manifest and the README are package metadata, not glue, so they carry
/// no such header.
#[test]
fn every_template_names_the_transport_contract() {
    for language in Language::ALL {
        for file in package(language, "my-ext") {
            if !is_glue(file.role) {
                continue;
            }
            assert!(
                file.contents.contains(GLUE_SOURCE),
                "{}: {} does not name the transport contract",
                language.as_str(),
                file.path
            );
        }
    }
}

/// Every MCP revision literal a generated file spells (`YYYY-MM-DD`) in
/// `body` — the shape a hand-typed revision takes.
fn revision_literals(body: &str) -> Vec<&[u8]> {
    let bytes = body.as_bytes();
    (0..bytes.len().saturating_sub(9))
        .filter(|&at| {
            bytes[at..at + 10]
                .iter()
                .enumerate()
                .all(|(i, byte)| match i {
                    4 | 7 => *byte == b'-',
                    _ => byte.is_ascii_digit(),
                })
        })
        .map(|at| &bytes[at..at + 10])
        .collect()
}

/// The revision a scaffold ships is the one this host serves: every revision
/// literal in a generated file is the wire kit's constant — a hand-typed
/// stale one would ship a handshake its own host refuses — and the manifest
/// spells it.
#[test]
fn every_template_speaks_this_hosts_revision() {
    let expected = MCP_PROTOCOL_VERSION.as_bytes();
    for language in Language::ALL {
        let files = package(language, "my-ext");
        let manifest = &with_role(&files, Role::Manifest)[0].contents;
        assert!(
            !revision_literals(manifest).is_empty(),
            "{}: the manifest spells no protocol revision",
            language.as_str()
        );
        for file in &files {
            for literal in revision_literals(&file.contents) {
                assert_eq!(
                    literal,
                    expected,
                    "{}: {} spells a revision other than this host's {MCP_PROTOCOL_VERSION}",
                    language.as_str(),
                    file.path
                );
            }
        }
    }
}

/// The envelope's flag reaches a Cypher parameter in every language: the
/// generated walk reads `name` off the envelope and binds it, and the
/// greeting statement carries the parameter rather than a spliced value.
#[test]
fn every_template_binds_the_sample_flag_as_a_parameter() {
    for language in Language::ALL {
        let files = package(language, "my-ext");
        let glue: String = files
            .iter()
            .filter(|file| is_glue(file.role))
            .map(|file| file.contents.as_str())
            .collect();
        // The same statement in every language; bash escapes the `$` of the
        // Cypher parameter so its own shell does not expand it.
        for fragment in ["RETURN 'Hello ' ", "$name", "+ ' world!' AS msg"] {
            assert!(
                glue.contains(fragment),
                "{}: the greeting statement is missing {fragment:?}",
                language.as_str()
            );
        }
        // Each language reads the flag from its own envelope source: the
        // decoded envelope (py/ts/rust) or the kit's flag reader (bash).
        assert!(
            glue.contains("envelope") || glue.contains("mcp_flag_string"),
            "{}: the walk never reads its flag from the envelope",
            language.as_str()
        );
        assert!(
            glue.contains("parameters"),
            "{}: the call carries no parameters object",
            language.as_str()
        );
    }
}

/// Every README is a quickstart: the two commands that install and run the
/// package, the read-back query, and the pointer to the author guide.
#[test]
fn every_template_readme_is_a_working_quickstart() {
    for language in Language::ALL {
        let files = package(language, "my-ext");
        let readme = &with_role(&files, Role::Readme)[0].contents;
        for needle in [
            "relate extension install my-ext --from .",
            "relate run my-ext --name Alex -d ./store",
            "relate query --database ./store",
            "docs/api/extensions/authoring.md",
        ] {
            assert!(
                readme.contains(needle),
                "{}: the README is missing {needle:?}",
                language.as_str()
            );
        }
    }
}

/// The roles are data: every role describes itself, and only the two glue
/// roles are glue.
#[test]
fn every_role_describes_itself() {
    for role in Role::ALL {
        assert!(!role.description().is_empty(), "{role:?}");
    }
    let glue: Vec<Role> = Role::ALL
        .into_iter()
        .filter(|role| is_glue(*role))
        .collect();
    assert_eq!(glue, vec![Role::GlueExecutable, Role::GlueSource]);
}

/// The `--lang` vocabulary is closed and every value round-trips; the
/// default is `py`.
#[test]
fn the_language_vocabulary_is_closed_and_py_is_the_default() {
    assert_eq!(Language::DEFAULT, Language::Py);
    assert_eq!(Language::vocabulary(), "sh, py, ts or rust");
    for language in Language::ALL {
        assert_eq!(Language::parse(language.as_str()), Some(language));
    }
    assert_eq!(Language::parse("python"), None);
    assert_eq!(Language::parse(""), None);
}
