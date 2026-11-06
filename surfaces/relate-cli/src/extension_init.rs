//! `relate extension init <name> [--lang sh|py|ts|rust]` (ADR-0431
//! §3): scaffold a package directory an author can install and run
//! immediately.
//!
//! The generated directory is the package contract's own shape — the
//! manifest plus one executable ([`crate::extensions::registry`]) — so
//! `extension install --from` and `relate run` work on it as they are.
//! The executable is the language's minimal MCP client, generated inline
//! ([`templates`]): the vendored glue, a TODO-marked `walk()`, and a README
//! quickstart. Nothing is fetched, nothing is published, and there is no
//! client dependency to install or manage.
//!
//! The verb is dependency-free, so it rides the `extensions` feature with
//! `list` and `remove` (§3); its grammar is parsed in that build like every
//! `extension` action (ADR-0137). A build without `extensions` answers the
//! whole command with the command-level not-included error (ADR-0104).
//!
//! Exit codes follow the CLI's split: a bad name, a reserved-word collision
//! or an unknown `--lang` is a usage error (exit 2, through the hand-rolled
//! parser's convention); an existing target directory or a filesystem
//! failure is a runtime failure (exit 1, one `Error:` line).

pub(crate) mod templates;

use std::path::{Path, PathBuf};

use crate::parse_error::ParseError;

use crate::extensions::registry::RegistryError;

use templates::{Language, PackageFile, Role};

/// The `init` action's parsed arguments — the same grammar in every build
/// that carries `extensions` (the ADR-0137 shape: parse ungated within the
/// command, gate the capability).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InitSpec {
    /// The package name: the mount name and the directory's name.
    pub(crate) name: String,
    /// The language to scaffold, already resolved to its template (the
    /// closed `--lang` vocabulary, `py` when the flag is absent).
    pub(crate) language: Language,
}

/// Parse the argument tail after `extension init`: one positional (the
/// package name) and `--lang` at most once. Pure — the usage-error message
/// is returned instead of exiting, so the grammar is unit-testable; the
/// lifecycle wrapper turns `Err` into the exit-2 convention.
pub(crate) fn parse_init_args(rest: &[String]) -> Result<InitSpec, ParseError> {
    use crate::parse_error::{flag_value, set_once};
    let mut name: Option<String> = None;
    let mut lang: Option<String> = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lang" => {
                let value = flag_value(&mut args, "--lang", "a value (sh, py, ts or rust)")?;
                set_once("extension init", &mut lang, "--lang", value.clone())?;
            }
            other if other.starts_with('-') => {
                return Err(ParseError::unexpected("extension init", other));
            }
            other => {
                if name.is_some() {
                    return Err(ParseError::unexpected("extension init", other));
                }
                name = Some(other.to_string());
            }
        }
    }
    let Some(name) = name else {
        return Err("extension init requires a package name".into());
    };
    validate_name(&name)?;
    // The closed `--lang` vocabulary: an unknown value is a usage error
    // naming the value and the vocabulary — never a silent fallback to the
    // default.
    let language = match lang {
        None => Language::DEFAULT,
        Some(word) => Language::parse(&word).ok_or_else(|| {
            ParseError::from(format!(
                "extension init: unknown --lang value '{word}' (expected {})",
                Language::vocabulary()
            ))
        })?,
    };
    Ok(InitSpec { name, language })
}

/// The package-name rule, consulted before anything is written: the name is
/// the directory `init` creates and the mount name the registry resolves, so
/// it must pass the one package-name predicate
/// ([`crate::extensions::is_package_name`] — the installer's rule, so a
/// scaffold is never a directory the installer would refuse), and it must not
/// be a reserved command word. The collision refusal is the registry's typed
/// one ([`RegistryError::Collision`]), byte for byte what a load-time
/// collision prints (ADR-0429 §6).
fn validate_name(name: &str) -> Result<(), ParseError> {
    if !crate::extensions::is_package_name(name) {
        return Err(format!(
            "extension init: '{name}' is not a valid package name (a single directory name, \
             no path separators)"
        )
        .into());
    }
    if let Some(word) = crate::flags::reserved_word(name) {
        return Err(RegistryError::reserved_word(name, word).into());
    }
    Ok(())
}

/// Scaffold the package and print next steps, returning the process exit
/// code (the pattern every lifecycle command module follows).
pub(crate) fn run(spec: InitSpec) -> i32 {
    // The scaffold's root is the working directory. A working directory the
    // platform cannot name is a runtime failure, never a guess.
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            crate::print_error(format!(
                "Error: extension init: resolving the working directory: {err}"
            ));
            return 1;
        }
    };
    match scaffold(&spec, &cwd) {
        Ok((dir, files)) => {
            print_created(&spec, &dir, &files);
            0
        }
        Err(err) => {
            crate::print_error(format!("Error: extension init: {err}"));
            1
        }
    }
}

/// Write `spec`'s package under `cwd` and return its directory: a new
/// directory only (an existing one is the refusal — `init` never merges
/// into or clobbers an author's files), and nothing half-written survives a
/// failure (the directory is removed again).
fn scaffold(spec: &InitSpec, cwd: &Path) -> Result<(PathBuf, Vec<PackageFile>), String> {
    let dir = cwd.join(&spec.name);
    if dir.exists() {
        return Err(format!(
            "{} already exists (init writes a new package directory)",
            dir.display()
        ));
    }
    std::fs::create_dir(&dir).map_err(|err| format!("creating {}: {err}", dir.display()))?;
    // Rendered once: the same files are written and printed.
    let files = spec.language.package(&spec.name);
    let written = (|| -> Result<(), String> {
        for file in &files {
            let path = dir.join(&file.path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("creating {}: {err}", parent.display()))?;
            }
            std::fs::write(&path, &file.contents)
                .map_err(|err| format!("writing {}: {err}", path.display()))?;
            if file.role == Role::GlueExecutable {
                make_executable(&path)?;
            }
        }
        Ok(())
    })();
    if let Err(err) = written {
        // No half-scaffolded package: the directory this call created goes,
        // so a failed init leaves the working directory as it found it. A
        // cleanup that also fails is named in the error — a stranded
        // half-scaffold is never silent.
        return Err(match std::fs::remove_dir_all(&dir) {
            Ok(()) => err,
            Err(cleanup) => format!(
                "{err} (removing the half-written {} also failed: {cleanup})",
                dir.display()
            ),
        });
    }
    Ok((dir, files))
}

/// Give a generated executable its exec bit where the platform has one —
/// the installer copies the mode, so the package runs the moment it lands.
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut mode = std::fs::metadata(path)
        .map_err(|err| format!("reading {}: {err}", path.display()))?
        .permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(path, mode)
        .map_err(|err| format!("setting the exec bit on {}: {err}", path.display()))
}

/// Where the platform has no exec bit, the script's shebang is the whole
/// story: a no-op.
#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// The block `init` prints: what landed, then the two commands that make it
/// a graph. The install line names the absolute directory, so it runs from
/// wherever the author stands next.
fn print_created(spec: &InitSpec, dir: &Path, files: &[PackageFile]) {
    let width = files
        .iter()
        .map(|file| file.path.len())
        .max()
        .unwrap_or_default();
    println!(
        "created {} extension '{}' in {}",
        spec.language.as_str(),
        spec.name,
        dir.display()
    );
    for file in files {
        println!("  {:<width$}  {}", file.path, file.role.description());
    }
    println!();
    println!("next:");
    println!(
        "  relate extension install {} --from {}",
        spec.name,
        dir.display()
    );
    println!("  relate run {} -d ./store", spec.name);
}

#[cfg(test)]
mod tests;
