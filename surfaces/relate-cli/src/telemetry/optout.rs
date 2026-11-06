//! Layered telemetry opt-out (ADR-0079 §6). First match wins:
//!
//! 1. `DO_NOT_TRACK` set to `1` or `true` (the consoledonottrack
//!    convention)
//! 2. `RELATE_TELEMETRY` (`0`/`false`/`off` disables; `1`/`true`/`on`
//!    enables; unrecognized values fall through)
//! 3. `telemetry: false` (or `true`) in `<config>/relate/config.json`
//! 4. default: enabled
//!
//! The environment lookup is injected (`&dyn Fn(&str) -> Option<String>`)
//! so the precedence matrix is unit-testable without mutating process env.

/// Which layer decided the effective telemetry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// `DO_NOT_TRACK=1|true`.
    DoNotTrack,
    /// `RELATE_TELEMETRY` held a recognized on/off value.
    Env,
    /// `config.json` held a `telemetry` bool.
    ConfigFile,
    /// Nothing was set anywhere; telemetry defaults to on.
    Default,
}

/// The resolved telemetry state plus the layer that decided it (surfaced by
/// `relate telemetry status`, ADR-0079 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveState {
    pub enabled: bool,
    pub layer: Layer,
}

impl EffectiveState {
    fn enabled(layer: Layer) -> Self {
        Self {
            enabled: true,
            layer,
        }
    }

    fn disabled(layer: Layer) -> Self {
        Self {
            enabled: false,
            layer,
        }
    }
}

/// Resolve the effective state. `env` looks up an environment variable
/// (returning `None` when unset); `config_telemetry` is the parsed
/// `telemetry` key from `config.json` (`None` when absent/corrupt).
pub(crate) fn resolve(
    env: &dyn Fn(&str) -> Option<String>,
    config_telemetry: Option<bool>,
) -> EffectiveState {
    if let Some(v) = env("DO_NOT_TRACK") {
        let v = v.trim();
        if v == "1" || v.eq_ignore_ascii_case("true") {
            return EffectiveState::disabled(Layer::DoNotTrack);
        }
    }
    if let Some(v) = env("RELATE_TELEMETRY") {
        match parse_on_off(&v) {
            Some(false) => return EffectiveState::disabled(Layer::Env),
            Some(true) => return EffectiveState::enabled(Layer::Env),
            // Unrecognized value: fall through to the next layer.
            None => {}
        }
    }
    match config_telemetry {
        Some(false) => EffectiveState::disabled(Layer::ConfigFile),
        Some(true) => EffectiveState::enabled(Layer::ConfigFile),
        None => EffectiveState::enabled(Layer::Default),
    }
}

/// Parse an on/off value: `0`/`false`/`off` disable, `1`/`true`/`on`
/// enable (trimmed, case-insensitive). Anything else is `None`.
fn parse_on_off(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "0" | "false" | "off" => Some(false),
        "1" | "true" | "on" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::env_of;

    #[test]
    fn nothing_set_means_enabled_by_default() {
        let s = resolve(&env_of(&[]), None);
        assert_eq!(
            s,
            EffectiveState {
                enabled: true,
                layer: Layer::Default
            }
        );
    }

    #[test]
    fn do_not_track_one_or_true_disables() {
        for v in ["1", "true", "TRUE", " True "] {
            let s = resolve(&env_of(&[("DO_NOT_TRACK", v)]), None);
            assert_eq!(
                s,
                EffectiveState {
                    enabled: false,
                    layer: Layer::DoNotTrack
                },
                "DO_NOT_TRACK={v:?}"
            );
        }
    }

    #[test]
    fn do_not_track_beats_config_true() {
        let s = resolve(&env_of(&[("DO_NOT_TRACK", "1")]), Some(true));
        assert_eq!(
            s,
            EffectiveState {
                enabled: false,
                layer: Layer::DoNotTrack
            }
        );
    }

    #[test]
    fn do_not_track_beats_relate_telemetry_on() {
        let s = resolve(
            &env_of(&[("DO_NOT_TRACK", "1"), ("RELATE_TELEMETRY", "1")]),
            None,
        );
        assert_eq!(
            s,
            EffectiveState {
                enabled: false,
                layer: Layer::DoNotTrack
            }
        );
    }

    #[test]
    fn do_not_track_other_values_fall_through() {
        for v in ["0", "false", "yes", ""] {
            let s = resolve(&env_of(&[("DO_NOT_TRACK", v)]), None);
            assert_eq!(
                s,
                EffectiveState {
                    enabled: true,
                    layer: Layer::Default
                },
                "DO_NOT_TRACK={v:?}"
            );
        }
    }

    #[test]
    fn relate_telemetry_off_values_disable() {
        for v in ["0", "false", "off", "OFF", " False "] {
            let s = resolve(&env_of(&[("RELATE_TELEMETRY", v)]), None);
            assert_eq!(
                s,
                EffectiveState {
                    enabled: false,
                    layer: Layer::Env
                },
                "RELATE_TELEMETRY={v:?}"
            );
        }
    }

    #[test]
    fn relate_telemetry_on_values_enable() {
        for v in ["1", "true", "on", "ON", " True "] {
            let s = resolve(&env_of(&[("RELATE_TELEMETRY", v)]), None);
            assert_eq!(
                s,
                EffectiveState {
                    enabled: true,
                    layer: Layer::Env
                },
                "RELATE_TELEMETRY={v:?}"
            );
        }
    }

    #[test]
    fn relate_telemetry_zero_beats_config_true() {
        let s = resolve(&env_of(&[("RELATE_TELEMETRY", "0")]), Some(true));
        assert_eq!(
            s,
            EffectiveState {
                enabled: false,
                layer: Layer::Env
            }
        );
    }

    #[test]
    fn relate_telemetry_on_beats_config_false() {
        let s = resolve(&env_of(&[("RELATE_TELEMETRY", "on")]), Some(false));
        assert_eq!(
            s,
            EffectiveState {
                enabled: true,
                layer: Layer::Env
            }
        );
    }

    #[test]
    fn unrecognized_env_value_falls_through_to_config() {
        let s = resolve(&env_of(&[("RELATE_TELEMETRY", "bogus")]), Some(false));
        assert_eq!(
            s,
            EffectiveState {
                enabled: false,
                layer: Layer::ConfigFile
            }
        );
    }

    #[test]
    fn config_file_false_disables() {
        let s = resolve(&env_of(&[]), Some(false));
        assert_eq!(
            s,
            EffectiveState {
                enabled: false,
                layer: Layer::ConfigFile
            }
        );
    }

    #[test]
    fn config_file_true_enables() {
        let s = resolve(&env_of(&[]), Some(true));
        assert_eq!(
            s,
            EffectiveState {
                enabled: true,
                layer: Layer::ConfigFile
            }
        );
    }
}
