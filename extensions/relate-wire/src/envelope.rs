//! The request envelope's shared child facts (ADR-0442 §1, ADR-0447): the
//! one JSON line the host writes on the child's stdin before the MCP
//! handshake, and the one thing every first-party child needs from it —
//! the `DIR` positional's resolution.

/// Resolve the `DIR` positional both first-party children declare in their
/// manifests: absent means the current directory the host left the child
/// in; a path that is not a directory is a refusal with the pair's one
/// wording. The envelope's positional semantics live here, not re-authored
/// per child.
pub fn resolve_dir(dir: Option<String>) -> Result<std::path::PathBuf, String> {
    let dir = match dir {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::env::current_dir()
            .map_err(|e| format!("cannot resolve the current directory: {e}"))?,
    };
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", dir.display()));
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{display, file_name};
    use std::path::Path;

    #[test]
    fn a_directory_resolves_and_a_non_directory_refuses() {
        // The absent spelling is the child's own cwd, which the spawn set.
        assert!(resolve_dir(None).is_ok());
        // The test binary's own path is a file: the pair's one refusal.
        let file = std::env::current_exe().expect("the test binary has a path");
        assert_eq!(
            resolve_dir(Some(file.display().to_string())).unwrap_err(),
            format!("not a directory: {}", file.display())
        );
        let dir = std::env::temp_dir();
        assert_eq!(resolve_dir(Some(dir.display().to_string())).unwrap(), dir);
    }

    /// The stored-path rendering: the two importers' one rendering, pinned
    /// so the fs and git children store the same bytes (ADR-0376 §5).
    #[test]
    fn the_stored_path_rendering_is_shared() {
        assert_eq!(display(Path::new("/w/alpha.txt")), "/w/alpha.txt");
        assert_eq!(file_name(Path::new("/w/alpha.txt")), "alpha.txt");
        assert_eq!(file_name(Path::new("/w/alpha.txt/")), "alpha.txt");
        // A path with no final component renders whole — the walked root.
        assert_eq!(file_name(Path::new("/")), "/");
    }
}
