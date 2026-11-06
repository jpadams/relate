//! Telemetry state under the CLI's config directory (ADR-0079 §3, §6):
//! `$XDG_CONFIG_HOME/relate/` when set, otherwise
//! `~/.config/relate/` — the shared resolution in
//! [`crate::config_dir`], the one fact behind every config-dir walk.
//!
//! - `state.json` holds `install_id` (a random UUID v4, generated once and
//!   reused thereafter) and `notice_shown` (one-time stderr notice flag).
//! - `config.json` holds the persistent opt-out key `telemetry` (bool).
//!
//! Writes are atomic-ish (write a temp file in the same directory, then
//! rename). Reads tolerate missing or corrupt files by regenerating /
//! treating keys as absent — never by crashing.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::atomic_file;
use crate::telemetry::envelope;

/// State filename holding the install ID and notice flag.
const STATE_FILE: &str = "state.json";

/// Config filename holding the persistent `telemetry` opt-out key.
const CONFIG_FILE: &str = "config.json";

/// Contents of `state.json` (ADR-0079 §3, §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Random UUID v4, generated once and reused thereafter.
    pub install_id: String,
    /// Whether the one-time first-run notice has been printed.
    pub notice_shown: bool,
}

/// Path of `state.json` inside a config directory.
pub fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE)
}

/// Path of `config.json` inside a config directory.
pub fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

/// Load `state.json`, or generate a fresh install ID and persist it when the
/// file is missing or corrupt. The install ID is therefore stable across
/// runs of the same config directory. Persistence failure is not fatal: the
/// fresh state is still returned (it just won't be stable).
pub fn load_or_create_state(dir: &Path) -> State {
    if let Some(state) = read_state(dir) {
        return state;
    }
    let state = State {
        install_id: envelope::uuid_v4(),
        notice_shown: false,
    };
    let _ = save_state(dir, &state);
    state
}

/// Persist `state.json` (atomic-ish: temp file + rename).
pub fn save_state(dir: &Path, state: &State) -> io::Result<()> {
    let body = serde_json::json!({
        "install_id": state.install_id,
        "notice_shown": state.notice_shown,
    });
    write_atomic(&state_path(dir), body.to_string().as_bytes())
}

/// The `telemetry` key from `config.json`, or `None` when the file is
/// missing, corrupt, or has no boolean `telemetry` key.
pub fn load_config_telemetry(dir: &Path) -> Option<bool> {
    let text = fs::read_to_string(config_path(dir)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("telemetry")?.as_bool()
}

/// Persist the `telemetry` key in `config.json` (atomic-ish). `config.json`
/// holds only this key today; the layout is load-bearing per ADR-0079.
pub fn save_config_telemetry(dir: &Path, enabled: bool) -> io::Result<()> {
    let body = serde_json::json!({ "telemetry": enabled });
    write_atomic(&config_path(dir), body.to_string().as_bytes())
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

/// Persist `bytes` to `path` through the shared hardened atomic-write helper,
/// with the 0600 permission bits `state.json` / `config.json` require: both
/// hold private data (the install UUID and the opt-out choice).
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_file::write_atomic(path, bytes, 0o600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_id_is_generated_once_and_reused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("relate");
        let first = load_or_create_state(&dir);
        assert!(is_uuid_shape(&first.install_id));
        assert!(!first.notice_shown);
        assert!(state_path(&dir).is_file());

        let second = load_or_create_state(&dir);
        assert_eq!(first, second, "install_id must be stable across loads");
    }

    #[test]
    fn state_roundtrip_preserves_notice_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let mut state = load_or_create_state(dir.as_path());
        state.notice_shown = true;
        save_state(dir.as_path(), &state).unwrap();
        let reloaded = load_or_create_state(dir.as_path());
        assert_eq!(reloaded, state);
    }

    #[test]
    fn corrupt_state_file_is_regenerated_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        fs::write(state_path(&dir), "not json at all").unwrap();
        let state = load_or_create_state(&dir);
        assert!(is_uuid_shape(&state.install_id));
        // The regenerated state replaces the corrupt file.
        let reloaded = load_or_create_state(&dir);
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
        let state = load_or_create_state(&dir);
        assert!(is_uuid_shape(&state.install_id));
        assert!(!state.notice_shown);
    }

    #[test]
    fn load_config_telemetry_missing_or_corrupt_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        assert_eq!(load_config_telemetry(&dir), None);
        fs::create_dir_all(&dir).unwrap();
        fs::write(config_path(&dir), "garbage").unwrap();
        assert_eq!(load_config_telemetry(&dir), None);
        fs::write(config_path(&dir), r#"{"other": true}"#).unwrap();
        assert_eq!(load_config_telemetry(&dir), None);
        fs::write(config_path(&dir), r#"{"telemetry": "yes"}"#).unwrap();
        assert_eq!(load_config_telemetry(&dir), None);
    }

    #[test]
    fn config_telemetry_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        save_config_telemetry(&dir, false).unwrap();
        assert_eq!(load_config_telemetry(&dir), Some(false));
        save_config_telemetry(&dir, true).unwrap();
        assert_eq!(load_config_telemetry(&dir), Some(true));
    }

    #[test]
    fn write_atomic_leaves_no_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        save_config_telemetry(&dir, true).unwrap();
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "only config.json should remain");
    }

    #[cfg(unix)]
    #[test]
    fn written_state_and_config_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let state = load_or_create_state(&dir);
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
