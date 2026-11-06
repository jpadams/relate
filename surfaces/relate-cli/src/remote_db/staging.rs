//! The remote `--load` fetch and its staged-file lifecycle (LABS-872,
//! ADR-0496) — the half of [`super`] that does I/O, compiled only with the
//! `remote-database` feature. The classification half (`super::plan`) stays
//! ungated and pure, so every build refuses a remote URL correctly before
//! this module is ever reachable.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::atomic_file::create_staging_file;
use crate::url_fetch::{get_into, FetchLimits};

/// Cap and timeout for one remote bundle fetch (1 GiB, 300 s global) — a
/// database is not a parameter document, so the knobs are this flag's own,
/// deliberately independent of `--fetch-params`'s (the ADR-0267 precedent
/// for independently set fetch knobs).
const MAX_BUNDLE_BYTES: u64 = 1024 * 1024 * 1024;
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// The staging name's prefix and suffix: the shared non-predictable recipe
/// with the bundle suffix the engine's single-file detection needs — the
/// extension this release writes (ADR-0517 §5).
const STAGING_PREFIX: &str = "relate-remote";
const BUNDLE_SUFFIX: &str = ".rl";

/// Fetch `url` and stage the bytes at a private temp `.rl` path — always
/// scratch, registered for the exit/open removal ([`remove_staged`],
/// through super's `cleanup_staged` entry) before its first byte: every
/// `--load` consumer releases the copy (the memory turn-into after the
/// load, the durable-target clone after the clone), so nothing is ever
/// kept. `https_only` is the caller's redirect-downgrade pin
/// ([`super::RemoteDb::https_only`]).
pub(super) fn fetch_and_stage(url: &str, https_only: bool) -> Result<PathBuf, String> {
    let limits = FetchLimits {
        cap_bytes: MAX_BUNDLE_BYTES,
        timeout: FETCH_TIMEOUT,
        https_only,
    };
    // Stage first, then stream the body straight into the file: a bundle at
    // the cap never lives in RAM (the rust review's memory finding). The
    // scratch copy registers for exit cleanup before its first byte lands;
    // a failed fetch removes its own partial file directly — the rollback
    // belongs to the fetch that owns the failed file.
    let (path, mut file) = stage_new_bundle()?;
    if let Err(f) = get_into(url, &limits, &mut file) {
        // Close the handle before removing: on Windows an open handle
        // blocks the removal (the same discipline `write_atomic` documents
        // for its rename), and a swallowed sharing violation would leak the
        // partial file. The rollback is the one removal policy's
        // ([`remove_staged`]) — the registry entry outlives a failed
        // removal, never the reverse.
        drop(file);
        remove_staged();
        return Err(f.message(&format!(
            "the remote database exceeded the {} GiB cap",
            MAX_BUNDLE_BYTES / (1024 * 1024 * 1024)
        )));
    }
    drop(file);
    Ok(path)
}

/// Create the one staged bundle file this process owns — the shared
/// exclusive-create step in the system temp dir under the `.rl` suffix —
/// and register it for [`remove_staged`]: the copy is scratch, whatever
/// `--load` disposition asked for it.
fn stage_new_bundle() -> Result<(PathBuf, std::fs::File), String> {
    let dir = std::env::temp_dir();
    let (path, file) = create_staging_file(&dir, STAGING_PREFIX, BUNDLE_SUFFIX, 0o600)
        .map_err(|e| format!("staging the remote bundle: {e}"))?;
    register_staged(&path);
    Ok((path, file))
}

/// The one staged scratch bundle this process owns — `process::exit` skips
/// destructors, so the exit path (`exit_flushed`) reads this instead of a
/// Drop guard.
static STAGED: Mutex<Option<PathBuf>> = Mutex::new(None);

fn register_staged(path: &Path) {
    STAGED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(path.to_path_buf());
}

/// The one removal policy for the staged copy (round 4, F-3-5), called by
/// the failed-fetch rollback and the exit/open cleanup (mod.rs's
/// `cleanup_staged` entry) alike: attempt the unlink, clear the
/// registration only on `Ok`, keep it on failure — a Windows sharing
/// violation leaves the entry registered so a later cleanup can retry,
/// and the partial file is never left unregistered.
pub(super) fn remove_staged() {
    let mut staged = STAGED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(path) = staged.as_ref() {
        if std::fs::remove_file(path).is_ok() {
            *staged = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The staged-copy registry is process-global, so the tests that
    /// touch it serialize on this lock instead of racing each other's
    /// registrations (a replace wins, and the loser's cleanup removes the
    /// winner's file).
    static REGISTRY: Mutex<()> = Mutex::new(());

    #[test]
    fn the_staging_name_is_unique_and_ends_in_the_bundle_extension() {
        let a = crate::atomic_file::staging_name(STAGING_PREFIX, BUNDLE_SUFFIX);
        let b = crate::atomic_file::staging_name(STAGING_PREFIX, BUNDLE_SUFFIX);
        assert!(a.ends_with(".rl"), "{a}");
        assert_ne!(a, b);
        assert!(!a.contains('/'), "a file name, not a path: {a}");
    }

    #[test]
    fn staging_writes_the_bytes_and_scratch_is_removed() {
        use std::io::Write;

        let _guard = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        // Every staged copy is scratch now (the kept-copy disposition died
        // with the `--database` URL spelling): removed on cleanup.
        let (path, mut file) = stage_new_bundle().unwrap();
        file.write_all(b"bundle-bytes").unwrap();
        drop(file);
        assert!(path.is_file());
        remove_staged();
        assert!(!path.exists(), "a scratch copy must be removed");

        // A second cleanup with nothing staged is a no-op.
        remove_staged();
    }

    #[test]
    fn a_failed_removal_keeps_the_registration_so_cleanup_can_retry() {
        use std::io::Write;

        let _guard = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        // Stage a copy, then delete the file out from under the registry:
        // the removal fails, so the entry must survive…
        let (path, mut file) = stage_new_bundle().unwrap();
        file.write_all(b"bundle-bytes").unwrap();
        drop(file);
        std::fs::remove_file(&path).unwrap();
        remove_staged();
        // …observable as the retry succeeding once the file exists again
        // (the same path, re-created): a taken-then-failed cleanup would
        // have deregistered it and left this second copy behind.
        std::fs::write(&path, b"bundle-bytes").unwrap();
        remove_staged();
        assert!(!path.exists(), "the retry removed the staged copy");
    }
}
