//! The extension registry (ADR-0429 §6): the XDG discovery walk, the
//! load-time refusals, and the installed-action enumeration.
//!
//! The registry walks `$XDG_CONFIG_HOME/relate/extensions/` and
//! nothing else — never the current working directory, never a
//! project-local directory. That is the drive-by-execution guard: the
//! grammar a `relate` invocation accepts is a property of the
//! user's machine, never of the directory the command happens to run
//! in, so cloning a checkout can never add executables to the grammar
//! (ADR-0429 §6).
//!
//! The registry's one source is the XDG walk: the first-party importers
//! are installed packages like any other, so the `run` mount resolves
//! every name through it, and the discovery error is the only answer an
//! uninstalled name gets.
//!
//! Every load-time violation refuses the load loudly, with a pinned
//! text naming the offender — never skipped, never first-wins, never
//! silent (ADR-0429 §6): a name colliding with a reserved command
//! word or another installed extension; a malformed manifest; an
//! executable path escaping its package directory. A refusal is an
//! exit-2 usage error at the CLI surface (ADR-0442's manifest class).

use std::fs;
use std::path::{Path, PathBuf};

use crate::extensions::manifest::Manifest;

/// The manifest filename every extension package directory carries
/// (ADR-0429 §2) — `pub(crate)` so the test kit's installs join through
/// the same fact the walk reads.
pub(crate) const MANIFEST_FILE: &str = "extension.json";

/// The extensions directory's name under the config dir (ADR-0429 §2) —
/// `pub(crate)` for the same reason: the kit's fixture installs and every
/// test's hand-built walk edges join through the walk's own segment.
pub(crate) const EXTENSIONS_DIR: &str = "extensions";

/// The extensions directory under the resolved config dir (ADR-0429 §2,
/// §6): the one path the discovery walk walks and the install verbs write
/// — the shared fact, so the walk and the installer can never disagree
/// about where packages live. The real-env twin of
/// [`extensions_dir_with`], reached through the config-dir rule's own
/// real-env twin ([`crate::config_dir::config_dir`]).
pub(crate) fn extensions_dir() -> Option<PathBuf> {
    crate::config_dir::config_dir().map(|config| config.join(EXTENSIONS_DIR))
}

/// Injectable-env variant of [`extensions_dir`], the
/// `config_dir`/`config_dir_with` pattern: the same resolution with the
/// base read through `env`, so the walk's own tests and the registry's
/// injectable loader pin it against a fixture environment. The join
/// lives here and in [`extensions_dir`] alone — the walk and the install
/// verbs consult the helpers, never a join of their own. Gated with its
/// only consumer ([`ActionRegistry::load_with`]): the non-test build
/// never carries the injector.
#[cfg(test)]
fn extensions_dir_with(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    crate::config_dir::config_dir_with(env).map(|config| config.join(EXTENSIONS_DIR))
}

/// One manifest-loaded command extension, as the registry holds it —
/// and as the `run` mount carries it: the verb's parse resolves the
/// action through the registry and hands the entry on, owned, so the
/// dispatch runs the action the invocation resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtensionEntry {
    /// The package directory the manifest was decoded from — the
    /// `<name>/` directory under the XDG extensions dir. The
    /// executable resolves only inside it (ADR-0429 §2).
    pub(crate) package_dir: PathBuf,
    /// The decoded manifest.
    pub(crate) manifest: Manifest,
}

impl ExtensionEntry {
    /// The extension's mount name — the `<name>` of `run <name>`
    /// (ADR-0429 §2): the manifest's `name`, the word dispatch
    /// resolves and the collision refusals check.
    pub(crate) fn mount_name(&self) -> &str {
        &self.command().name
    }

    /// The command-stratum payload — the manifest half the host parses
    /// and runs from: the flag table, the positional, the lifecycle
    /// knobs. The single v1 stratum; a new kind lands as a new arm here,
    /// and the closed vocabulary has already refused everything else at
    /// decode.
    pub(crate) fn command(&self) -> &crate::extensions::manifest::CommandManifest {
        let Manifest::Command(command) = &self.manifest;
        command
    }
}

/// The registry of installed actions (ADR-0429): ONE enumeration of the
/// machine's installed command-kind extensions — the manifest-loaded
/// rows the XDG discovery walk loads. The one table behind the action
/// grammar: the `run` mount resolves through it and its listing reads
/// it, hidden rows filtered by the verb's own listing policy.
pub(crate) struct ActionRegistry {
    /// The manifest-loaded rows, in the discovery walk's sorted order.
    actions: Vec<ExtensionEntry>,
}

impl ActionRegistry {
    /// Load the registry: walk the XDG extensions dir (§6 — never the
    /// CWD, never project-local), decode every package's manifest
    /// strictly, refuse every load-time violation loudly. The real-env
    /// entry point: the walk's root is the canonical
    /// [`extensions_dir`] resolution.
    pub(crate) fn load() -> Result<Self, RegistryError> {
        Ok(Self {
            actions: discover(extensions_dir())?,
        })
    }

    /// Injectable-env variant of [`Self::load`], for tests: the walk's
    /// root resolves through the same shared rule, injectably. Gated to
    /// test builds — every consumer is a test — so the non-test build
    /// never carries the injector.
    #[cfg(test)]
    pub(crate) fn load_with(env: &dyn Fn(&str) -> Option<String>) -> Result<Self, RegistryError> {
        Ok(Self {
            actions: discover(extensions_dir_with(env))?,
        })
    }

    /// Resolve a word: the one lookup the dispatch paths go through. An
    /// extension's mount name yields its manifest-loaded row; anything
    /// else is `None`.
    pub(crate) fn find(&self, word: &str) -> Option<&ExtensionEntry> {
        self.actions.iter().find(|entry| entry.mount_name() == word)
    }

    /// The manifest-loaded rows, in load order — the `run` mount's
    /// view: its dispatch resolves through it (via
    /// [`Self::find`]) and its installed-actions listing reads it,
    /// hidden rows filtered by the verb's own listing policy.
    pub(crate) fn extension_actions(&self) -> impl Iterator<Item = &ExtensionEntry> + '_ {
        self.actions.iter()
    }
}

/// A load-time refusal (ADR-0429 §6): every variant renders a pinned
/// text naming the offender; the parser surfaces it as the exit-2
/// usage error (ADR-0442's manifest class).
#[derive(Debug)]
pub(crate) enum RegistryError {
    /// The extensions directory itself could not be read (a missing
    /// one is the ship-with-zero state, not an error).
    Walk {
        /// The read failure.
        source: std::io::Error,
    },
    /// A package's manifest failed the strict decode: malformed JSON,
    /// an unknown kind, an unknown field, a mistyped default —
    /// fail-closed, never skipped (ADR-0429 §2-§3).
    Malformed {
        /// The package's directory name.
        package: String,
        /// The decode failure, naming the offending value.
        source: serde_json::Error,
    },
    /// A package's manifest exists but could not be read (a missing
    /// manifest is [`Self::MissingManifest`]).
    Unreadable {
        /// The package's directory name.
        package: String,
        /// The read failure.
        source: std::io::Error,
    },
    /// The package directory has no `extension.json`.
    MissingManifest {
        /// The package's directory name.
        package: String,
    },
    /// A manifest's name collides (ADR-0429 §6): the load refuses with
    /// a pinned error naming both sides — never first-wins, never
    /// silent.
    Collision {
        /// The refused extension's name.
        name: String,
        /// What it collides with.
        other: CollidesWith,
    },
    /// The manifest's executable path escapes the package directory
    /// (ADR-0429 §2): the executable resolves only inside the
    /// package dir.
    EscapingExecutable {
        /// The package's directory name.
        package: String,
        /// The escaping path, as written in the manifest.
        executable: String,
    },
}

/// The other side of a name collision (ADR-0429 §6): a reserved
/// top-level command word or an already-installed extension.
#[derive(Debug)]
pub(crate) enum CollidesWith {
    /// A reserved command word ([`crate::flags::reserved_word`],
    /// ADR-0069).
    ReservedWord(&'static str),
    /// An already-installed extension, named by its package directory.
    InstalledExtension(String),
}

impl RegistryError {
    /// The reserved-word collision refusal (ADR-0429 §6) — the one
    /// constructor its three sites share: the registry's load-time check, the
    /// install flow's pre-flight, and `extension init`'s name rule.
    pub(crate) fn reserved_word(name: &str, word: &'static str) -> Self {
        Self::Collision {
            name: name.to_string(),
            other: CollidesWith::ReservedWord(word),
        }
    }
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Walk { source } => {
                write!(f, "cannot read the extensions directory: {source}")
            }
            Self::Malformed { package, source } => {
                write!(f, "extension '{package}': invalid manifest: {source}")
            }
            Self::Unreadable { package, source } => {
                write!(f, "extension '{package}': cannot read extension.json: {source}")
            }
            Self::MissingManifest { package } => write!(
                f,
                "extension '{package}': no extension.json in the package directory"
            ),
            Self::Collision { name, other } => {
                write!(f, "extension name '{name}' collides with {other}")
            }
            Self::EscapingExecutable { package, executable } => write!(
                f,
                "extension '{package}': executable path '{executable}' escapes the package directory"
            ),
        }
    }
}

impl std::fmt::Display for CollidesWith {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReservedWord(word) => {
                write!(f, "the reserved command word '{word}'")
            }
            Self::InstalledExtension(package) => {
                write!(f, "the already-installed extension (package '{package}')")
            }
        }
    }
}

impl From<RegistryError> for crate::parse_error::ParseError {
    /// A load refusal becomes the parser's usage error: the rendered
    /// refusal text, exit 2 at the CLI surface (ADR-0442).
    fn from(err: RegistryError) -> Self {
        err.to_string().into()
    }
}

/// Discover the installed extensions (ADR-0429 §6): every package
/// directory directly under the XDG extensions dir — nothing else.
/// Deterministic: the walk sorts the packages, so the enumeration and
/// every refusal are stable. The refusals are load-time: a collision,
/// a malformed manifest, or an escaping executable refuses the whole
/// load — never a skipped package, never first-wins. The caller hands
/// the walk its root from the canonical extensions-dir resolution
/// ([`extensions_dir`] real, [`extensions_dir_with`] injected) — the
/// same helper the install verbs write through, so the two cannot
/// disagree about where packages live.
fn discover(extensions_dir: Option<PathBuf>) -> Result<Vec<ExtensionEntry>, RegistryError> {
    // `None`: neither base resolves — nothing to walk.
    let Some(extensions_dir) = extensions_dir else {
        return Ok(Vec::new());
    };
    let read = match fs::read_dir(&extensions_dir) {
        Ok(read) => read,
        // The missing directory is the ship-with-zero state (ADR-0429
        // §7): zero installed extensions, an empty extension half.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(RegistryError::Walk { source: e }),
    };
    // Only directories are packages, and never an installer's staged
    // sibling (the `.relate-stage-*` convention, ADR-0431 §4 step 4):
    // residue of a crashed install is not a package — the walk skips it,
    // the next install sweeps it. Every other entry is accounted for: a
    // failed read refuses the walk, never a silently dropped package.
    let mut packages: Vec<PathBuf> = read
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| RegistryError::Walk { source })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| {
            path.is_dir() && {
                // The staging skip: our own mid-install residue.
                path.file_name()
                    .is_none_or(|name| !crate::staging::is_staging_name(name))
            }
        })
        .collect();
    packages.sort();

    let mut installed: Vec<ExtensionEntry> = Vec::new();
    for package_dir in packages {
        // Every per-package refusal — the decode, the escaping executable
        // and the reserved command word — is the loader's; the walk adds
        // the one refusal only a set can make: a duplicate mount name,
        // naming both sides (ADR-0429 §6).
        let entry = load_package(&package_dir)?;
        let name = entry.mount_name();
        if let Some(other) = installed.iter().find(|entry| entry.mount_name() == name) {
            return Err(RegistryError::Collision {
                name: name.to_string(),
                other: CollidesWith::InstalledExtension(package_name(&other.package_dir)),
            });
        }
        installed.push(entry);
    }
    Ok(installed)
}

/// Load one package: read its `extension.json`, decode it strictly,
/// check the executable stays inside the package directory, and refuse a
/// mount name that spells a reserved command word. One loader for every
/// caller that reads a single package directory — the discovery walk and
/// the `extension check` verb (`extensions::check`) — so a package fails
/// the same way whether the machine loads it or an author checks it. The
/// refusal texts name the package by its directory name.
pub(crate) fn load_package(package_dir: &Path) -> Result<ExtensionEntry, RegistryError> {
    let package = package_name(package_dir);
    let manifest_path = package_dir.join(MANIFEST_FILE);
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RegistryError::MissingManifest { package });
        }
        Err(e) => return Err(RegistryError::Unreadable { package, source: e }),
    };
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|source| RegistryError::Malformed {
            package: package.clone(),
            source,
        })?;
    // The executable resolves only inside the package dir (ADR-0429
    // §2): an absolute or parent-escaping path is a load refusal — the
    // extensions root's one containment predicate.
    let Manifest::Command(command) = &manifest;
    if super::escapes_package_dir(&command.executable) {
        return Err(RegistryError::EscapingExecutable {
            package,
            executable: command.executable.display().to_string(),
        });
    }
    // The reserved-word collision (ADR-0429 §6): an installed name never
    // enters the CLI's command-word vocabulary, so a manifest that
    // spells one is refused at load — the package's own property, hence
    // here rather than in the walk.
    if let Some(word) = crate::flags::reserved_word(&command.name) {
        return Err(RegistryError::reserved_word(&command.name, word));
    }
    Ok(ExtensionEntry {
        package_dir: package_dir.to_owned(),
        manifest,
    })
}

/// The package's name in refusal texts: its directory's name — every
/// walked path came from `read_dir`, so it has one. The one projection
/// of a package directory's name (the registry's load refusals and the
/// install flow's collision texts both render through it), so no caller
/// re-derives the name with a fallback of its own.
pub(crate) fn package_name(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .expect("a read_dir entry path always has a file name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_dir::CONFIG_DIR_NAME;
    use crate::telemetry::env_of;

    /// The extension tree's shared kit builds the fixture packages the
    /// discovery tests walk: `install_package` joins through this
    /// module's own consts (`CONFIG_DIR_NAME`, `EXTENSIONS_DIR`,
    /// `MANIFEST_FILE`), so a fixture install and the walk under test
    /// cannot disagree about where a package lives or what it carries.
    use crate::extensions::testkit::{install_package, manifest_for};

    /// `load_with` against `root` as `$XDG_CONFIG_HOME`.
    fn load(root: &Path) -> Result<ActionRegistry, RegistryError> {
        let xdg = root.to_string_lossy().into_owned();
        let pairs = [("XDG_CONFIG_HOME", xdg.as_str())];
        let env = env_of(&pairs);
        ActionRegistry::load_with(&env)
    }

    /// The rendered refusal of a load that must fail.
    fn refusal(load: Result<ActionRegistry, RegistryError>) -> String {
        match load {
            Err(err) => err.to_string(),
            Ok(_) => panic!("expected a load refusal"),
        }
    }

    #[test]
    fn no_config_base_is_the_empty_registry() {
        let registry = ActionRegistry::load_with(&env_of(&[])).unwrap();
        assert_eq!(registry.extension_actions().count(), 0);
    }

    #[test]
    fn a_missing_extensions_directory_is_the_empty_registry() {
        let tmp = tempfile::tempdir().unwrap();
        // `$XDG_CONFIG_HOME` exists, `relate/extensions/` does not:
        // the ship-with-zero state (ADR-0429 §7), not an error.
        let registry = load(tmp.path()).unwrap();
        assert_eq!(registry.extension_actions().count(), 0);
    }

    #[test]
    fn discovery_reads_packages_in_name_order() {
        let tmp = tempfile::tempdir().unwrap();
        install_package(&tmp, "zeta", &manifest_for("zeta", "", ""));
        install_package(&tmp, "alpha", &manifest_for("alpha", "", ""));
        let registry = load(tmp.path()).unwrap();
        let names: Vec<&str> = registry
            .extension_actions()
            .map(|e| e.mount_name())
            .collect();
        assert_eq!(names, ["alpha", "zeta"]);
    }

    /// The walk stays inside the extensions directory (ADR-0429 §6):
    /// a package beside it — under the same config dir — and a manifest
    /// as a bare file inside it are not extensions.
    #[test]
    fn the_walk_reads_only_directories_under_the_extensions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        install_package(&tmp, "inside", &manifest_for("inside", "", ""));
        // Beside the extensions dir, under the config dir: not a package.
        let beside = tmp.path().join(CONFIG_DIR_NAME).join("outside");
        fs::create_dir_all(&beside).unwrap();
        fs::write(beside.join(MANIFEST_FILE), manifest_for("outside", "", "")).unwrap();
        // A bare file inside the extensions dir: not a package.
        fs::write(
            tmp.path()
                .join(CONFIG_DIR_NAME)
                .join(EXTENSIONS_DIR)
                .join("loose.json"),
            manifest_for("loose", "", ""),
        )
        .unwrap();
        let registry = load(tmp.path()).unwrap();
        let names: Vec<&str> = registry
            .extension_actions()
            .map(|e| e.mount_name())
            .collect();
        assert_eq!(names, ["inside"]);
    }

    #[test]
    fn a_package_without_a_manifest_refuses_the_load() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            tmp.path()
                .join(CONFIG_DIR_NAME)
                .join(EXTENSIONS_DIR)
                .join("ghost"),
        )
        .unwrap();
        let err = refusal(load(tmp.path()));
        assert_eq!(
            err,
            "extension 'ghost': no extension.json in the package directory"
        );
    }

    #[test]
    fn a_malformed_manifest_refuses_the_load_naming_the_package() {
        let tmp = tempfile::tempdir().unwrap();
        install_package(&tmp, "broken", "{ nope");
        let err = refusal(load(tmp.path()));
        assert!(
            err.starts_with("extension 'broken': invalid manifest: "),
            "{err}"
        );
    }

    /// The closed-vocabulary gate at load (ADR-0429 §3): a manifest
    /// naming a future stratum's kind is a refusal naming the package
    /// and the value — never a future-proofed install.
    #[test]
    fn an_unknown_kind_refuses_the_load_naming_the_package_and_value() {
        let tmp = tempfile::tempdir().unwrap();
        install_package(
            &tmp,
            "future",
            &manifest_for("future", "", "").replacen("\"command\"", "\"callables\"", 1),
        );
        let err = refusal(load(tmp.path()));
        assert!(
            err.starts_with("extension 'future': invalid manifest: "),
            "{err}"
        );
        assert!(err.contains("unknown variant"), "{err}");
    }

    #[test]
    fn a_name_colliding_with_a_reserved_word_refuses_the_load() {
        for word in ["serve", "query", "run"] {
            let tmp = tempfile::tempdir().unwrap();
            install_package(&tmp, "helper", &manifest_for(word, "", ""));
            let err = refusal(load(tmp.path()));
            assert_eq!(
                err,
                format!("extension name '{word}' collides with the reserved command word '{word}'"),
                "{word} is a reserved command word (ADR-0069)"
            );
        }
    }

    #[test]
    fn a_name_colliding_with_an_installed_extension_refuses_the_load() {
        let tmp = tempfile::tempdir().unwrap();
        install_package(&tmp, "alpha", &manifest_for("dup", "", ""));
        install_package(&tmp, "beta", &manifest_for("dup", "", ""));
        let err = refusal(load(tmp.path()));
        // Both sides named: the refused name and the package that
        // already installed it (the walk's sorted order makes the
        // refusal deterministic).
        assert_eq!(
            err,
            "extension name 'dup' collides with the already-installed extension (package 'alpha')"
        );
    }

    #[test]
    fn an_escaping_executable_refuses_the_load() {
        // Portable escapes: an absolute path and parent-directory
        // components (the `Prefix` component of a `C:\…` path is the
        // windows spelling of the same refusal).
        for escaping in ["../evil.sh", "/absolute/path.sh", "a/../../evil.sh"] {
            let tmp = tempfile::tempdir().unwrap();
            let manifest = manifest_for("escape", "", "").replacen(
                "\"run.sh\"",
                &format!("\"{escaping}\""),
                1,
            );
            install_package(&tmp, "escape", &manifest);
            let err = refusal(load(tmp.path()));
            assert_eq!(
                err,
                format!(
                    "extension 'escape': executable path '{escaping}' escapes the package directory"
                )
            );
        }
    }

    #[test]
    fn the_walk_skips_staged_siblings_of_a_crashed_install() {
        // ADR-0431 §4 step 4: a `.relate-stage-*` sibling is an
        // installer's residue, never a package — the walk skips it (a
        // staged sibling has no manifest, so reading it as a package
        // would refuse the whole load). The next install sweeps it.
        let tmp = tempfile::tempdir().unwrap();
        install_package(&tmp, "greet", &manifest_for("greet", "", ""));
        let staged = tmp
            .path()
            .join(CONFIG_DIR_NAME)
            .join(EXTENSIONS_DIR)
            .join(".relate-stage-123-456");
        fs::create_dir_all(&staged).unwrap();
        let registry = load(tmp.path()).unwrap();
        let names: Vec<&str> = registry
            .extension_actions()
            .map(|e| e.mount_name())
            .collect();
        assert_eq!(names, ["greet"]);
    }

    /// The one lookup the dispatch paths go through: an extension's
    /// mount name resolves; anything else is `None` — no built-in words
    /// exist to shadow an install.
    #[test]
    fn find_resolves_an_installed_mount_name_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        install_package(&tmp, "greet", &manifest_for("greet", "", ""));
        let registry = load(tmp.path()).unwrap();
        assert!(registry.find("greet").is_some());
        assert!(registry.find("bogus").is_none());
    }
}
