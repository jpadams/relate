//! The staging-directory convention (the `skill install` staging shape,
//! ADR-0261, generalized by ADR-0431 §4 step 4): an installer that lands a
//! directory stages into a non-predictable `.relate-stage-*` sibling
//! *inside* the destination directory — same filesystem, so the final
//! rename is atomic — and a crashed install never leaves a half-written
//! package at the destination path.
//!
//! The atomic replace parks the previous install under a distinct
//! `.relate-replaced-*` sibling while the new package lands (ADR-0431
//! §4 step 4), so neither the age sweep nor the registry walk mistakes a
//! package mid-replace for a crashed staging directory. Both prefixes are
//! the installer's own temporary siblings: the walks skip them, and
//! [`sweep_stale`] reclaims only the ones old enough to be abandoned.
//!
//! Compiled when any of its consumers is: `skill` (the skills dirs) or
//! `extensions` (the XDG extensions dir — the registry's walk skips the
//! prefix even in builds without `extension-install`, so a crashed
//! install's residue never reads as a package).

// The path types the installer-only helpers below take; gated with them
// so the prefix-only build (bare `extensions`: the registry consults the
// predicate, no installer is compiled) carries no unused import.
#[cfg(any(feature = "skill", feature = "extension-install"))]
use std::path::{Path, PathBuf};
#[cfg(any(feature = "skill", feature = "extension-install"))]
use std::time::{Duration, SystemTime};

/// The staging-name prefix: ours by naming convention (ADR-0261;
/// ADR-0431 §4 step 4).
pub(crate) const STAGING_PREFIX: &str = ".relate-stage-";

/// The parked-aside prefix: the atomic replace's previous install, moved
/// aside while the new package lands. Distinct from [`STAGING_PREFIX`] so
/// the sweep never reclaims a package that is only mid-replace (ADR-0431
/// §4 step 4).
pub(crate) const REPLACED_PREFIX: &str = ".relate-replaced-";

/// How long an installer temporary sibling must sit untouched before
/// [`sweep_stale`] may treat it as a crashed install's residue. A live
/// install's staging directory — and, mid-replace, the parked-aside
/// previous install — is fresh; only a sibling older than this is
/// unambiguously abandoned.
#[cfg(any(feature = "skill", feature = "extension-install"))]
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// A non-predictable sibling name under `prefix`: pid plus a process-local
/// counter plus the current nanosecond, so two installs in one process —
/// or two processes in one nanosecond — never collide, and a hostile
/// co-tenant cannot pre-create the path.
#[cfg(any(feature = "skill", feature = "extension-install"))]
fn unique_sibling_name(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the system clock is set before the Unix epoch")
        .as_nanos();
    format!("{prefix}{}-{seq}-{nanos}", std::process::id())
}

/// A unique staging directory *name* (join it into the destination dir).
#[cfg(any(feature = "skill", feature = "extension-install"))]
pub(crate) fn staging_dir_name() -> String {
    unique_sibling_name(STAGING_PREFIX)
}

/// A unique parked-aside directory *name* (join it into the extensions
/// dir): the same non-predictability rule as [`staging_dir_name`].
#[cfg(any(feature = "skill", feature = "extension-install"))]
pub(crate) fn replaced_dir_name() -> String {
    unique_sibling_name(REPLACED_PREFIX)
}

/// Whether `name` is an installer temporary sibling's name — a staging
/// directory or the parked-aside previous install of an atomic replace.
/// This is the one predicate the walks that must skip in-flight siblings
/// consult, so neither prefix ever reads as a package.
pub(crate) fn is_staging_name(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with(STAGING_PREFIX) || name.starts_with(REPLACED_PREFIX)
}

/// Best-effort removal of stale installer siblings in `dir` — staging
/// residue of a crashed install and a parked-aside previous install left
/// by a crash mid-replace. Only entries older than [`STALE_AFTER`] are
/// reclaimed: a live install's sibling (another process's, or this one's
/// parked previous package) is recent, and an entry whose age cannot be
/// read is kept on the conservative side. Individual removal errors are
/// ignored (the caller is about to stage its own sibling, and a stuck
/// residue will be visible on the next manual look).
#[cfg(any(feature = "skill", feature = "extension-install"))]
pub(crate) fn sweep_stale(dir: &Path) {
    sweep_stale_at(dir, SystemTime::now());
}

/// [`sweep_stale`] against an explicit clock — the age rule's seam, so the
/// test can age siblings without touching their mtimes.
#[cfg(any(feature = "skill", feature = "extension-install"))]
fn sweep_stale_at(dir: &Path, now: SystemTime) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if !is_staging_name(&entry.file_name()) {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= STALE_AFTER);
            if stale {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

/// Put a parked original back at `target` — the one restore step of the
/// park-aside replace policy (both directory installs speak it, ADR-0431
/// §3): a restore that fails strands the original in its parked-aside
/// sibling, and the caller's `strand` text names both failures and where
/// the only copy now lives, rather than silently dropping it.
#[cfg(any(feature = "skill", feature = "extension-install"))]
pub(crate) fn restore_parked<E>(
    target: &Path,
    aside: &Path,
    strand: impl FnOnce(std::io::Error) -> E,
) -> Result<(), E> {
    std::fs::rename(aside, target).map_err(strand)
}

/// Land `staged` over `target`, whose previous content was parked at
/// `aside` — the one landing policy of the park-aside replace: a failed
/// landing puts the parked original back before refusing (a failed replace
/// never leaves the name half-written), a restore that fails too strands
/// the original (the caller's `strand` text names the landing error, the
/// restore error, and where the only copy now lives), and a successful
/// landing sweeps the parked copy best-effort — a sweep failure must not
/// fail a landed replace (`sweep_stale` reclaims it once it ages).
///
/// Between parking and landing the target briefly does not exist (POSIX
/// cannot rename over a non-empty directory): the replace is
/// crash-recoverable and never half-written, but not gap-free for a
/// concurrent reader.
#[cfg(any(feature = "skill", feature = "extension-install"))]
pub(crate) fn land_parked<E>(
    target: &Path,
    staged: &Path,
    aside: &Path,
    land: impl FnOnce(std::io::Error) -> E,
    strand: impl FnOnce(std::io::Error, std::io::Error) -> E,
) -> Result<(), E> {
    if let Err(e) = std::fs::rename(staged, target) {
        if let Err(restore) = std::fs::rename(aside, target) {
            return Err(strand(e, restore));
        }
        return Err(land(e));
    }
    let _ = std::fs::remove_dir_all(aside);
    Ok(())
}

/// The staging path inside `dir`: the destination directory joined with
/// the unique staging name.
#[cfg(any(feature = "skill", feature = "extension-install"))]
pub(crate) fn staging_dir(dir: &Path) -> PathBuf {
    dir.join(staging_dir_name())
}

/// The parked-aside path inside `dir`: the extensions directory joined
/// with the unique replaced name the atomic replace moves the previous
/// install to.
#[cfg(any(feature = "skill", feature = "extension-install"))]
pub(crate) fn replaced_dir(dir: &Path) -> PathBuf {
    dir.join(replaced_dir_name())
}

#[cfg(all(test, any(feature = "skill", feature = "extension-install")))]
mod tests {
    use super::*;

    /// The age rule: a fresh sibling survives (it may be a live install's
    /// staging dir or a package mid-replace), while the same sibling under
    /// a later clock is reclaimed — under either prefix. A package
    /// directory is never a candidate.
    #[test]
    fn sweep_stale_reclaims_only_aged_siblings() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join(staging_dir_name());
        std::fs::create_dir(&staged).unwrap();
        let parked = tmp.path().join(replaced_dir_name());
        std::fs::create_dir(&parked).unwrap();
        let package = tmp.path().join("greet");
        std::fs::create_dir(&package).unwrap();

        // At the real clock both siblings are fresh: neither is residue.
        sweep_stale_at(tmp.path(), SystemTime::now());
        assert!(staged.exists(), "a fresh staging dir is a live install's");
        assert!(parked.exists(), "a fresh parked-aside dir is mid-replace");

        // Two hours on, both cross the threshold and are reclaimed.
        sweep_stale_at(
            tmp.path(),
            SystemTime::now() + Duration::from_secs(2 * 60 * 60),
        );
        assert!(!staged.exists(), "an aged staging dir is residue");
        assert!(!parked.exists(), "an aged parked-aside dir is residue");
        assert!(package.exists(), "a package directory is never swept");
    }
}
