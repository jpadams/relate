//! Agent catalog and path expansion, ported from neo4j-cli's
//! `common/skill/agents.go`. The catalog lists the skill-capable AI agents
// (their detect directory and skills directory). Path expansion mirrors the
// Go `expandPath`: `~` and `~/foo` resolve via $HOME; `$XDG_CONFIG_HOME/...`
// resolves via that env var or $HOME/.config when unset/empty; other paths
// pass through unchanged.

use std::path::PathBuf;

/// Describes a supported AI agent: where its install marker lives
/// (`detect_dir`) and where its skill bundles go (`skills_dir`).
///
/// `detect_dir` and `skills_dir` are stored in their unexpanded form
/// (with `~` or `$XDG_CONFIG_HOME`); call [`Agent::detect_path`] /
/// [`Agent::skills_path`] to resolve.
#[derive(Debug, PartialEq)]
pub struct Agent {
    /// Canonical lowercase id, e.g. "claude-code".
    pub name: &'static str,
    /// Human-readable name, e.g. "Claude Code".
    pub display_name: &'static str,
    /// Unexpanded path used to detect agent presence.
    detect_dir: &'static str,
    /// Unexpanded path where skill bundles are placed.
    skills_dir: &'static str,
}

impl Agent {
    /// Returns the expanded `detect_dir`, or `None` when the base env var
    /// (e.g. $HOME) can't be resolved.
    pub fn detect_path(&self) -> Option<PathBuf> {
        expand_path(self.detect_dir)
    }

    /// Returns the expanded `skills_dir`, or `None` when the base env var
    /// can't be resolved. All 11 catalog entries have a skills dir.
    pub fn skills_path(&self) -> Option<PathBuf> {
        expand_path(self.skills_dir)
    }
}

/// The supported agent catalog. Order is preserved for stable list output.
/// Exactly the 11 skill-capable entries from neo4j-cli; the MCP-only
/// claude-desktop entry is intentionally excluded.
pub const AGENTS: &[Agent] = &[
    Agent {
        name: "claude-code",
        display_name: "Claude Code",
        detect_dir: "~/.claude",
        skills_dir: "~/.claude/skills",
    },
    Agent {
        name: "cursor",
        display_name: "Cursor",
        detect_dir: "~/.cursor",
        skills_dir: "~/.cursor/skills",
    },
    Agent {
        name: "windsurf",
        display_name: "Windsurf",
        detect_dir: "~/.codeium/windsurf",
        skills_dir: "~/.codeium/windsurf/skills",
    },
    Agent {
        name: "copilot",
        display_name: "Copilot",
        detect_dir: "~/.copilot",
        skills_dir: "~/.copilot/skills",
    },
    Agent {
        name: "antigravity",
        display_name: "Antigravity",
        detect_dir: "~/.gemini/antigravity",
        skills_dir: "~/.gemini/antigravity/skills",
    },
    Agent {
        name: "gemini-cli",
        display_name: "Gemini CLI",
        detect_dir: "~/.gemini",
        skills_dir: "~/.gemini/skills",
    },
    Agent {
        name: "cline",
        display_name: "Cline",
        detect_dir: "~/.cline",
        skills_dir: "~/.agents/skills",
    },
    Agent {
        name: "codex",
        display_name: "Codex",
        detect_dir: "~/.codex",
        skills_dir: "~/.codex/skills",
    },
    Agent {
        name: "pi",
        display_name: "Pi",
        detect_dir: "~/.pi/agent",
        skills_dir: "~/.pi/agent/skills",
    },
    Agent {
        name: "opencode",
        display_name: "OpenCode",
        detect_dir: "$XDG_CONFIG_HOME/opencode",
        skills_dir: "$XDG_CONFIG_HOME/opencode/skills",
    },
    Agent {
        name: "junie",
        display_name: "Junie",
        detect_dir: "~/.junie",
        skills_dir: "~/.junie/skills",
    },
];

/// Returns only agents whose expanded `detect_dir` exists on the real
/// filesystem, in `AGENTS` order.
pub fn detect_agents() -> Vec<&'static Agent> {
    AGENTS
        .iter()
        .filter(|a| a.detect_path().map(|p| p.is_dir()).unwrap_or(false))
        .collect()
}

/// Looks up an agent by name, case-insensitive. Returns `None` for an
/// unknown name.
pub fn find_agent(name: &str) -> Option<&'static Agent> {
    let lower = name.to_ascii_lowercase();
    AGENTS.iter().find(|a| a.name == lower)
}

/// Returns the 11 catalog names, for error listings.
pub fn agent_names() -> Vec<&'static str> {
    AGENTS.iter().map(|a| a.name).collect()
}

const TOKEN_XDG_CONFIG_HOME: &str = "$XDG_CONFIG_HOME";

/// Resolves a path containing `~` or `$XDG_CONFIG_HOME`.
///
/// - `~` alone -> $HOME
/// - `~/foo`   -> $HOME/foo
/// - `$XDG_CONFIG_HOME/...` -> that env var, or $HOME/.config when unset/empty
/// - other paths are returned unchanged
///
/// Returns `None` only when the base the path needs can't be resolved
/// (e.g. $HOME empty).
fn expand_path(path: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();

    if path == "~" {
        if home.is_empty() {
            return None;
        }
        return Some(PathBuf::from(home));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if home.is_empty() {
            return None;
        }
        return Some(PathBuf::from(home).join(rest));
    }
    if path.contains(TOKEN_XDG_CONFIG_HOME) {
        let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_default();
        let base = if !xdg.is_empty() {
            xdg
        } else if !home.is_empty() {
            format!("{home}/.config")
        } else {
            return None;
        };
        return Some(PathBuf::from(path.replace(TOKEN_XDG_CONFIG_HOME, &base)));
    }
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Env-var tests mutate process-global env vars, so serialize them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env_lock<F: FnOnce()>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        f();
    }

    #[test]
    fn catalog_has_eleven_entries_in_order() {
        assert_eq!(AGENTS.len(), 11);
        let names: Vec<_> = AGENTS.iter().map(|a| a.name).collect();
        assert_eq!(
            names,
            vec![
                "claude-code",
                "cursor",
                "windsurf",
                "copilot",
                "antigravity",
                "gemini-cli",
                "cline",
                "codex",
                "pi",
                "opencode",
                "junie",
            ]
        );
    }

    #[test]
    fn skills_dir_values_match_neo4j_cli() {
        for a in AGENTS {
            // Every entry must have a skills dir.
            assert!(!a.skills_dir.is_empty(), "{} has empty skills_dir", a.name);
        }
        assert_eq!(AGENTS[6].skills_dir, "~/.agents/skills"); // cline
        assert_eq!(AGENTS[9].skills_dir, "$XDG_CONFIG_HOME/opencode/skills"); // opencode
    }

    #[test]
    fn find_agent_is_case_insensitive() {
        assert_eq!(
            find_agent("claude-code").map(|a| a.name),
            Some("claude-code")
        );
        assert_eq!(
            find_agent("Claude-Code").map(|a| a.name),
            Some("claude-code")
        );
        assert_eq!(
            find_agent("CLAUDE-CODE").map(|a| a.name),
            Some("claude-code")
        );
        assert_eq!(
            find_agent("Claude-CODE").map(|a| a.name),
            Some("claude-code")
        );
        assert_eq!(find_agent("unknown"), None);
    }

    #[test]
    fn detect_agents_filters_absent_dirs() {
        with_env_lock(|| {
            std::env::set_var("HOME", "/nonexistent-home-dir-xyz");
            std::env::remove_var("XDG_CONFIG_HOME");
            let detected = detect_agents();
            // No nonexistent dirs should be detected.
            assert!(
                detected.is_empty(),
                "expected no agents detected with absent dirs, got {:?}",
                detected.iter().map(|a| a.name).collect::<Vec<_>>()
            );
        });
    }

    #[test]
    fn detect_agents_finds_existing_dir() {
        with_env_lock(|| {
            // Create a temp home with a .claude dir.
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().to_path_buf();
            std::fs::create_dir_all(home.join(".claude")).unwrap();
            std::env::set_var("HOME", &home);
            std::env::remove_var("XDG_CONFIG_HOME");
            let detected = detect_agents();
            let names: Vec<_> = detected.iter().map(|a| a.name).collect();
            assert!(
                names.contains(&"claude-code"),
                "claude-code should be detected, got {names:?}"
            );
            // gemini-cli uses ~/.gemini which doesn't exist
            assert!(!names.contains(&"gemini-cli"));
        });
    }

    #[test]
    fn home_unset_returns_empty_detect_agents() {
        with_env_lock(|| {
            std::env::remove_var("HOME");
            std::env::remove_var("XDG_CONFIG_HOME");
            let detected = detect_agents();
            // opencode uses $XDG_CONFIG_HOME which falls back to $HOME/.config,
            // but HOME is unset so that's None too -> empty.
            assert!(
                detected.is_empty(),
                "expected empty with HOME unset, got {:?}",
                detected.iter().map(|a| a.name).collect::<Vec<_>>()
            );
        });
    }

    #[test]
    fn xdg_fallback_resolves_to_home_config() {
        with_env_lock(|| {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().to_path_buf();
            std::fs::create_dir_all(home.join(".config").join("opencode")).unwrap();
            std::env::set_var("HOME", &home);
            std::env::remove_var("XDG_CONFIG_HOME");

            // opencode detect_dir is $XDG_CONFIG_HOME/opencode -> $HOME/.config/opencode
            let opencode = find_agent("opencode").unwrap();
            let path = opencode.detect_path().unwrap();
            assert_eq!(path, home.join(".config").join("opencode"));
            assert!(path.is_dir());

            // detect_agents should now include opencode
            let detected = detect_agents();
            let names: Vec<_> = detected.iter().map(|a| a.name).collect();
            assert!(names.contains(&"opencode"));
        });
    }

    #[test]
    fn xdg_set_overrides_fallback() {
        with_env_lock(|| {
            let tmp = tempfile::tempdir().unwrap();
            let xdg = tmp.path().to_path_buf();
            std::fs::create_dir_all(xdg.join("opencode")).unwrap();
            std::env::set_var("HOME", "/nonexistent-home-dir-xyz");
            std::env::set_var("XDG_CONFIG_HOME", &xdg);

            let opencode = find_agent("opencode").unwrap();
            let path = opencode.detect_path().unwrap();
            assert_eq!(path, xdg.join("opencode"));
            assert!(path.is_dir());
        });
    }

    #[test]
    fn expand_path_tilde_alone() {
        with_env_lock(|| {
            std::env::set_var("HOME", "/tmp/fakehome");
            assert_eq!(expand_path("~"), Some(PathBuf::from("/tmp/fakehome")));
        });
    }

    #[test]
    fn expand_path_tilde_prefix() {
        with_env_lock(|| {
            std::env::set_var("HOME", "/tmp/fakehome");
            assert_eq!(
                expand_path("~/foo/bar"),
                Some(PathBuf::from("/tmp/fakehome/foo/bar"))
            );
        });
    }

    #[test]
    fn expand_path_plain() {
        with_env_lock(|| {
            assert_eq!(expand_path("/etc/hosts"), Some(PathBuf::from("/etc/hosts")));
        });
    }

    #[test]
    fn expand_path_home_empty_returns_none_for_tilde() {
        with_env_lock(|| {
            std::env::remove_var("HOME");
            assert_eq!(expand_path("~"), None);
            assert_eq!(expand_path("~/foo"), None);
        });
    }

    #[test]
    fn agent_names_returns_eleven() {
        assert_eq!(agent_names().len(), 11);
    }
}
