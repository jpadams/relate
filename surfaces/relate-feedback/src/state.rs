//! Telemetry state under the caller's config directory (ADR-0079 §3, §6;
//! ADR-0588 §1): `$XDG_CONFIG_HOME/relate/` when set, otherwise
//! `~/.config/relate/` — the directory is the caller's argument, the CLI
//! keeps the one config-directory rule.
//!
//! - `state.json` holds `install_id` (a random UUID v4, generated once and
//!   reused thereafter) and `notice_shown` (the one-time first-run-notice
//!   flag).
//! - `config.json` holds the persistent opt-out key `telemetry` (bool).
//!
//! Writes are atomic-ish (a non-predictable staging file in the same
//! directory, created exclusively with the mode at birth, then renamed over
//! the target). `config.json`'s read-edit-write is serialized by an
//! exclusive OS lock on `config.lock`, so the CLI and a running
//! `feedback serve` cannot drop each other's keys. `state.json` reads
//! tolerate a missing or corrupt file by regenerating; `config.json` reads
//! distinguish the two — a missing file is an empty map, an unreadable or
//! corrupt one a typed error — because an update from an empty map would
//! drop the keys it could not parse.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use fs4::FileExt;

use crate::ids::uuid_v4;

/// State filename holding the install ID and notice flag.
const STATE_FILE: &str = "state.json";

/// Config filename holding the persistent `telemetry` opt-out key.
const CONFIG_FILE: &str = "config.json";

/// Lock filename serializing the `config.json` read-edit-write.
const CONFIG_LOCK_FILE: &str = "config.lock";

/// Process-local sequence that makes each staging name unique even within
/// one process (two writes in the same nanosecond still differ).
static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Contents of `state.json` (ADR-0079 §3, §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Random UUID v4, generated once and reused thereafter.
    pub install_id: String,
    /// Whether the one-time first-run notice has been printed.
    pub notice_shown: bool,
}

impl State {
    /// One-time first-run-notice policy (ADR-0079 §6): `true` exactly once
    /// per install — on the first run with telemetry enabled, interactive
    /// or not — setting `notice_shown` as it does. The flag is set only when
    /// the record that makes the claim once-per-install was written: a write
    /// failure is returned so the caller can tell a claimed-and-recorded
    /// notice from one that will repeat.
    pub fn claim_notice(&mut self, dir: &Path) -> io::Result<bool> {
        if self.notice_shown {
            return Ok(false);
        }
        self.notice_shown = true;
        if let Err(error) = save_state(dir, self) {
            self.notice_shown = false;
            return Err(error);
        }
        Ok(true)
    }
}

/// Path of `state.json` inside a config directory.
pub fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE)
}

/// Path of `config.json` inside a config directory.
pub fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

/// Whether [`load_or_create_state`] could record the state it returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persistence {
    /// The state was read from, or written to, `state.json`: the install ID
    /// is stable across runs.
    Durable,
    /// The fresh state could not be recorded: it is returned anyway, and the
    /// install ID will change next run.
    Ephemeral,
}

/// Load `state.json`, or generate a fresh install ID and persist it when the
/// file is missing or corrupt. The install ID is therefore stable across
/// runs of the same config directory. A write failure is not fatal: the
/// fresh state is still returned with [`Persistence::Ephemeral`], so the
/// caller decides what a state it cannot record is worth.
pub fn load_or_create_state(dir: &Path) -> (State, Persistence) {
    if let Some(state) = read_state(dir) {
        return (state, Persistence::Durable);
    }
    let state = State {
        install_id: uuid_v4(),
        notice_shown: false,
    };
    let persistence = match save_state(dir, &state) {
        Ok(()) => Persistence::Durable,
        Err(_) => Persistence::Ephemeral,
    };
    (state, persistence)
}

/// Persist `state.json` (atomic-ish: staging file + rename).
pub fn save_state(dir: &Path, state: &State) -> io::Result<()> {
    let body = serde_json::json!({
        "install_id": state.install_id,
        "notice_shown": state.notice_shown,
    });
    write_atomic(&state_path(dir), body.to_string().as_bytes())
}

/// The parsed `config.json` object. A missing file is an empty map; an
/// unreadable or corrupt one is an [`io::ErrorKind::InvalidData`] error, so a
/// reader can tell "nothing recorded yet" from "this file cannot be trusted".
/// `config.json` holds more than one key (the telemetry opt-out and the
/// feedback consent record), so callers read and rewrite the whole object
/// instead of a single key's value.
pub fn load_config(dir: &Path) -> io::Result<serde_json::Map<String, serde_json::Value>> {
    let path = config_path(dir);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(serde_json::Map::new());
        }
        Err(error) => return Err(error),
    };
    serde_json::from_str(&text).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: {error}", path.display()),
        )
    })
}

/// Rewrite `config.json` (atomic-ish) after `edit` mutates the loaded
/// object: every other key survives the write. A file that cannot be parsed
/// is refused rather than overwritten from an empty map, because the write
/// would silently drop every key it could not read.
pub fn update_config(
    dir: &Path,
    edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) -> io::Result<()> {
    // The CLI and a long-running `feedback serve` both update this file; the
    // exclusive lock makes the read-edit-write atomic across processes (and
    // threads), so one writer cannot drop the other's keys. The OS releases
    // it when the holder exits, so a crash leaves no stale lock behind.
    let _lock = lock_config(dir)?;
    let mut config = load_config(dir)?;
    edit(&mut config);
    write_atomic(
        &config_path(dir),
        serde_json::Value::Object(config).to_string().as_bytes(),
    )
}

/// Take the exclusive update lock on `config.lock`, creating the config
/// directory when missing. Blocks until the current holder releases it.
fn lock_config(dir: &Path) -> io::Result<File> {
    fs::create_dir_all(dir)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(CONFIG_LOCK_FILE))?;
    file.lock_exclusive()?;
    Ok(file)
}

/// The `telemetry` key from `config.json`, keeping the one distinction the
/// opt-out ladder needs: nothing recorded, a value, or a file that cannot be
/// trusted. A corrupt file — or a `telemetry` key that is present but not a
/// boolean — must not read as "nothing recorded", which would fall through to
/// the enabled default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTelemetry {
    /// No file, or no `telemetry` key.
    Missing,
    /// The recorded opt-out value.
    Value(bool),
    /// The file cannot be read or parsed, or the key holds a non-boolean.
    Unreadable,
}

/// The typed [`ConfigTelemetry`] read of `config.json`.
pub fn load_config_telemetry(dir: &Path) -> ConfigTelemetry {
    let Ok(config) = load_config(dir) else {
        return ConfigTelemetry::Unreadable;
    };
    match config.get("telemetry") {
        None => ConfigTelemetry::Missing,
        Some(serde_json::Value::Bool(enabled)) => ConfigTelemetry::Value(*enabled),
        Some(_) => ConfigTelemetry::Unreadable,
    }
}

/// Persist the `telemetry` key in `config.json` (atomic-ish), keeping the
/// other keys (the feedback consent record) intact.
pub fn save_config_telemetry(dir: &Path, enabled: bool) -> io::Result<()> {
    update_config(dir, |config| {
        config.insert("telemetry".to_string(), enabled.into());
    })
}

fn read_state(dir: &Path) -> Option<State> {
    let text = fs::read_to_string(state_path(dir)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let install_id = value.get("install_id")?.as_str()?;
    if !is_uuid_shape(install_id) {
        return None;
    }
    let notice_shown = value
        .get("notice_shown")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Some(State {
        install_id: install_id.to_string(),
        notice_shown,
    })
}

/// Loose UUID shape check: 8-4-4-4-12 hex groups with dashes. Corrupt or
/// hand-mangled IDs are regenerated rather than trusted.
fn is_uuid_shape(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let lengths = [8, 4, 4, 4, 12];
    parts.len() == 5
        && parts
            .iter()
            .zip(lengths)
            .all(|(p, len)| p.len() == len && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Persist `bytes` to `path` atomic-ish: a non-predictable staging file in
/// the same directory, created exclusively with 0600 at birth (both state
/// files hold private data — the install UUID and the opt-out choice), then
/// renamed over the target; the parent directory is fsynced after the
/// rename.
///
/// Crate-local by necessity: the CLI's staged-file kit is not callable from
/// here (the CLI is this crate's consumer), so the state files' writer
/// states the same hardening once in the module that owns them.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("relate");
    let seq = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".{name}.tmp-{}-{seq}-{nanos}", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    let staged = file.write_all(bytes).and_then(|()| file.sync_all());
    // Close the staging file before renaming: on Windows an open handle
    // blocks the rename.
    drop(file);
    let result = staged
        .and_then(|()| fs::rename(&tmp, path))
        .and_then(|()| sync_parent_dir(dir));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// fsync the parent directory so the rename itself is durable. Non-unix
/// platforms cannot portably open a directory for syncing.
#[cfg(unix)]
fn sync_parent_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_id_is_generated_once_and_reused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("relate");
        let (first, persistence) = load_or_create_state(&dir);
        assert_eq!(persistence, Persistence::Durable);
        assert!(is_uuid_shape(&first.install_id));
        assert!(!first.notice_shown);
        assert!(state_path(&dir).is_file());

        let (second, _) = load_or_create_state(&dir);
        assert_eq!(first, second, "install_id must be stable across loads");
    }

    /// A state that cannot be written is returned with the fact that it is
    /// ephemeral, not silently reported as recorded.
    #[test]
    fn an_unrecordable_state_is_ephemeral() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        // A directory where state.json should be makes the write fail.
        fs::create_dir_all(state_path(&dir)).unwrap();
        let (state, persistence) = load_or_create_state(&dir);
        assert!(is_uuid_shape(&state.install_id));
        assert_eq!(persistence, Persistence::Ephemeral);
    }

    #[test]
    fn state_roundtrip_preserves_notice_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let (mut state, _) = load_or_create_state(dir.as_path());
        state.notice_shown = true;
        save_state(dir.as_path(), &state).unwrap();
        let (reloaded, _) = load_or_create_state(dir.as_path());
        assert_eq!(reloaded, state);
    }

    #[test]
    fn claim_notice_is_true_once_and_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let (mut state, _) = load_or_create_state(dir);
        assert!(
            state.claim_notice(dir).unwrap(),
            "first check shows the notice"
        );
        assert!(
            !state.claim_notice(dir).unwrap(),
            "second check is suppressed by the notice_shown flag"
        );
        // A later process (fresh load, same dir) sees the persisted flag.
        assert!(load_or_create_state(dir).0.notice_shown);
    }

    /// A claim whose record could not be written is an error, not a claim:
    /// the notice repeats on the next run instead of vanishing for good.
    #[test]
    fn an_unrecordable_claim_is_an_error_and_leaves_the_flag_unset() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        // A directory where state.json should be makes the write fail.
        fs::create_dir_all(state_path(&dir)).unwrap();
        let mut state = State {
            install_id: "123e4567-e89b-42d3-a456-426614174000".to_string(),
            notice_shown: false,
        };
        assert!(state.claim_notice(&dir).is_err());
        assert!(!state.notice_shown, "an unrecorded claim is not claimed");
    }

    #[test]
    fn corrupt_state_file_is_regenerated_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        fs::write(state_path(&dir), "not json at all").unwrap();
        let (state, _) = load_or_create_state(&dir);
        assert!(is_uuid_shape(&state.install_id));
        // The regenerated state replaces the corrupt file.
        let (reloaded, _) = load_or_create_state(&dir);
        assert_eq!(state, reloaded);
    }

    #[test]
    fn state_with_invalid_install_id_is_regenerated() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            state_path(&dir),
            r#"{"install_id": "not-a-uuid", "notice_shown": true}"#,
        )
        .unwrap();
        let (state, _) = load_or_create_state(&dir);
        assert!(is_uuid_shape(&state.install_id));
        assert!(!state.notice_shown);
    }

    /// Nothing recorded, a recorded value, and an untrusted value read three
    /// ways — and an untrusted `telemetry` value fails closed in the ladder
    /// exactly like a corrupt file: it never reads as "nothing recorded",
    /// which would re-enable telemetry.
    #[test]
    fn config_telemetry_read_distinguishes_missing_value_and_unreadable() {
        use crate::consent;
        use crate::test_support::env_of;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        assert_eq!(load_config_telemetry(&dir), ConfigTelemetry::Missing);
        fs::create_dir_all(&dir).unwrap();
        fs::write(config_path(&dir), "garbage").unwrap();
        assert_eq!(load_config_telemetry(&dir), ConfigTelemetry::Unreadable);
        fs::write(config_path(&dir), r#"{"other": true}"#).unwrap();
        assert_eq!(load_config_telemetry(&dir), ConfigTelemetry::Missing);
        for untrusted in [r#"{"telemetry": "yes"}"#, r#"{"telemetry": 0}"#] {
            fs::write(config_path(&dir), untrusted).unwrap();
            let recorded = load_config_telemetry(&dir);
            assert_eq!(recorded, ConfigTelemetry::Unreadable, "{untrusted}");
            assert!(
                !consent::resolve(&env_of(&[]), recorded).enabled(),
                "an untrusted telemetry value must not read as enabled: {untrusted}"
            );
        }
    }

    #[test]
    fn config_telemetry_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        save_config_telemetry(&dir, false).unwrap();
        assert_eq!(load_config_telemetry(&dir), ConfigTelemetry::Value(false));
        save_config_telemetry(&dir, true).unwrap();
        assert_eq!(load_config_telemetry(&dir), ConfigTelemetry::Value(true));
    }

    #[test]
    fn config_writes_preserve_other_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        fs::write(config_path(&dir), r#"{"feedback": {"endpoint": "x"}}"#).unwrap();
        save_config_telemetry(&dir, true).unwrap();
        let config = load_config(&dir).unwrap();
        assert_eq!(config["telemetry"], serde_json::json!(true));
        assert_eq!(config["feedback"]["endpoint"], serde_json::json!("x"));
    }

    /// A readable file and an absent one read differently; a corrupt one is
    /// an error rather than an empty map.
    #[test]
    fn config_reads_distinguish_missing_from_corrupt() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        assert!(load_config(&dir).unwrap().is_empty(), "missing is empty");
        fs::create_dir_all(&dir).unwrap();
        fs::write(config_path(&dir), "not json").unwrap();
        assert_eq!(
            load_config(&dir).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    /// The consent record lives in the same file: an update must not rewrite
    /// a `config.json` it could not parse from an empty map.
    #[test]
    fn updates_refuse_to_overwrite_an_unparsable_config() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        fs::write(config_path(&dir), "not json").unwrap();
        assert!(save_config_telemetry(&dir, true).is_err());
        assert_eq!(
            fs::read_to_string(config_path(&dir)).unwrap(),
            "not json",
            "the unparsable file is left untouched"
        );
    }

    #[test]
    fn write_atomic_leaves_no_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        save_config_telemetry(&dir, true).unwrap();
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["config.json", "config.lock"],
            "no staging file remains; the lock file is the only extra entry"
        );
    }

    /// Two writers must not lose each other's keys: a second update waits
    /// for the lock the first one holds.
    #[test]
    fn a_contended_config_update_waits_for_the_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let held = lock_config(&dir).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let writer_dir = dir.clone();
        let writer = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            update_config(&writer_dir, |config| {
                config.insert("telemetry".to_string(), true.into());
            })
            .unwrap();
            done_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "the second writer waits while the lock is held"
        );
        FileExt::unlock(&held).unwrap();
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the waiting update runs once the lock is free");
        writer.join().unwrap();
        assert_eq!(load_config_telemetry(&dir), ConfigTelemetry::Value(true));
    }

    #[cfg(unix)]
    #[test]
    fn written_state_and_config_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let (state, _) = load_or_create_state(&dir);
        save_state(&dir, &state).unwrap();
        save_config_telemetry(&dir, true).unwrap();
        for path in [state_path(&dir), config_path(&dir)] {
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{} must be 0600, got {mode:o}", path.display());
        }
    }

    #[test]
    fn uuid_shape_validation() {
        assert!(is_uuid_shape("123e4567-e89b-42d3-a456-426614174000"));
        assert!(is_uuid_shape("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"));
        assert!(!is_uuid_shape(""));
        assert!(!is_uuid_shape("not-a-uuid"));
        assert!(!is_uuid_shape("123e4567-e89b-42d3-a456-42661417400")); // short
        assert!(!is_uuid_shape("123e4567-e89b-42d3-a456-42661417400g")); // non-hex
        assert!(!is_uuid_shape("123e4567e89b42d3a456426614174000")); // no dashes
    }
}
