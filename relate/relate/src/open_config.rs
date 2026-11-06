//! Open-time option resolution: environment defaults for the snapshot codec,
//! memory limit, checkpoint interval, the commit durability policy
//! (ADR-0470), and the ADR-0209 bulk-write batch knobs. Checkpoint
//! execution stays in [`crate::checkpoint`].

use std::time::Duration;

use relate_core::{
    parse_expression_memory_limit, parse_memory_limit, parse_positive_memory_limit,
    require_positive_memory_limit, require_traversal_depth, PreviewFeatures, MAX_TRAVERSAL_DEPTH,
};
use relate_executor::PatternEngine;
use relate_storage::SnapshotCodecKind;

use crate::{DurabilityMode, Error, OpenOptions, Result, DEFAULT_CHECKPOINT_INTERVAL_SECS};

/// Read a non-blank environment value, folding unset, blank, and
/// `VarError::NotUnicode` into `None`. The preview resolver intentionally
/// does not use this helper: it folds `NotUnicode` into `None`, while
/// [`resolve_preview_features`] must fail open on a present-but-invalid
/// value (ADR-0371), so it reads `std::env::var` directly through
/// [`preview_features_from_setting`].
fn env_setting(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(s) if !s.trim().is_empty() => Some(s),
        _ => None,
    }
}

pub(crate) fn resolve_snapshot_codec(opts: &OpenOptions) -> Result<Option<SnapshotCodecKind>> {
    if let Some(kind) = opts.snapshot_codec {
        return Ok(Some(kind));
    }
    match env_setting("RELATE_SNAPSHOT_FORMAT") {
        Some(s) => Ok(Some(s.parse()?)),
        None => Ok(None),
    }
}

/// The markdown vault's body-property configuration (ADR-0460): the
/// `OpenOptions` value wins, else `RELATE_MARKDOWN_BODY_PROPERTIES`
/// parses with the flag's grammar — a bad value fails open with the
/// same error the flag's parse gives; unset or blank is `None`, the
/// sidecar manifest's recorded configuration.
pub(crate) fn resolve_markdown_bodies(
    opts: &OpenOptions,
) -> Result<Option<relate_storage::MarkdownBodyProperties>> {
    if let Some(bodies) = &opts.markdown_body_properties {
        return Ok(Some(bodies.clone()));
    }
    markdown_bodies_from_setting(std::env::var("RELATE_MARKDOWN_BODY_PROPERTIES"))
}

/// The one environment-value triage the variable-backed resolvers
/// share: unset, blank, and whitespace are `None`; a value that is not
/// valid UTF-8 fails open naming the variable — an invalid value is
/// never silently ignored, the preview-feature contract
/// ([`preview_features_from_setting`]). Pure in the raw result, so
/// tests never touch the process environment.
fn present_env_value(
    name: &'static str,
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<Option<String>> {
    match raw {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(crate::Error::semantic(format!(
            "invalid {name}: value is not valid UTF-8"
        ))),
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
    }
}

/// The markdown vault's body-property configuration from an already-read
/// `RELATE_MARKDOWN_BODY_PROPERTIES` value. Unset, blank, and
/// whitespace are `None` (the sidecar manifest's recorded configuration
/// governs); a present value parses with the flag's grammar. Pure in the
/// raw result, so tests never touch the process environment.
fn markdown_bodies_from_setting(
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<Option<relate_storage::MarkdownBodyProperties>> {
    let Some(spec) = present_env_value("RELATE_MARKDOWN_BODY_PROPERTIES", raw)? else {
        return Ok(None);
    };
    Ok(Some(relate_storage::MarkdownBodyProperties::parse(&spec)?))
}

/// Bytes from an already-read `RELATE_LOGICAL_GRAPH_SIZE_LIMIT` value.
/// `None` (unset or blank) is unlimited. A present value uses
/// [`parse_memory_limit`]; a bad value is that error, the same rejection
/// as `--logical-graph-size-limit`.
pub(crate) fn memory_limit_from_setting(raw: Option<&str>) -> Result<u64> {
    match raw {
        Some(s) => parse_memory_limit(s),
        None => Ok(0),
    }
}

/// A renamed limit variable must not resolve as unset. Unset is unlimited,
/// so a deployment still using the old name would open uncapped.
fn reject_renamed_env(old: &str, new: &str) -> Result<()> {
    renamed_env_error(old, new, std::env::var(old))
}

fn renamed_env_error(
    old: &str,
    new: &str,
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<()> {
    match raw {
        Err(std::env::VarError::NotPresent) => Ok(()),
        Ok(value) if value.trim().is_empty() => Ok(()),
        _ => Err(crate::Error::semantic(format!(
            "{old} is set but ignored; set {new}"
        ))),
    }
}

/// Old `CYPHERLITE_*` names whose mechanical `CYPHERLITE_`→`RELATE_`
/// spelling is itself a retired name (the pre-rename engine had already
/// renamed them once): each names its current spelling directly, so the
/// guard never points at a knob the engine also rejects.
fn current_spelling(old: &str) -> Option<&'static str> {
    match old {
        "CYPHERLITE_MEMORY_LIMIT" => Some("RELATE_LOGICAL_GRAPH_SIZE_LIMIT"),
        "CYPHERLITE_TOKEN_ADMISSION" => Some("RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT"),
        "CYPHERLITE_CONCURRENT_PEAK_TOKEN_LIMIT" => {
            Some("RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT")
        }
        _ => None,
    }
}

/// The product rename (ADR-0517): every `CYPHERLITE_*` knob became
/// `RELATE_*`. A still-set old name must not resolve as unset — unset is
/// the unlimited/default arm for the limit and safety knobs, so an
/// upgraded deployment would silently lose its configuration. One prefix
/// scan reuses the pair guard's fold; the prefix test is case-insensitive
/// because Windows environment lookup is, and three knobs the pre-rename
/// engine had already renamed once are guided to their *current*
/// spelling, not the mechanical intermediate the engine also rejects.
pub fn reject_cypherlite_env() -> Result<()> {
    // The scan iterates the whole environment. wasm32 has none —
    // `std::env::vars_os` is unsupported there and panics (the per-name
    // `std::env::var` reads every other resolver makes resolve as unset on
    // wasm) — and with no environment no `CYPHERLITE_*` name can be set,
    // so the guard is vacuously clean in the browser build.
    #[cfg(target_arch = "wasm32")]
    return Ok(());
    #[cfg(not(target_arch = "wasm32"))]
    for (raw_name, raw_value) in std::env::vars_os() {
        let Some(name) = raw_name.to_str() else {
            continue;
        };
        let upper = name.to_ascii_uppercase();
        if !upper.starts_with("CYPHERLITE_") {
            continue;
        }
        let new = current_spelling(&upper)
            .map(str::to_string)
            .unwrap_or_else(|| upper.replacen("CYPHERLITE_", "RELATE_", 1));
        let raw = match raw_value.to_str() {
            Some(value) => Ok(value.to_string()),
            None => Err(std::env::VarError::NotUnicode(raw_value)),
        };
        renamed_env_error(&upper, &new, raw)?;
    }
    Ok(())
}

pub(crate) fn resolve_memory_limit(opts: &OpenOptions) -> Result<u64> {
    reject_renamed_env("RELATE_MEMORY_LIMIT", "RELATE_LOGICAL_GRAPH_SIZE_LIMIT")?;
    match opts.logical_graph_size_limit {
        Some(n) => Ok(n),
        None => {
            memory_limit_from_setting(env_setting("RELATE_LOGICAL_GRAPH_SIZE_LIMIT").as_deref())
        }
    }
}

/// The traversal-depth admission limit from an already-read
/// `RELATE_MAX_TRAVERSAL_DEPTH` value (LABS-610). Unset or blank is the
/// default ([`MAX_TRAVERSAL_DEPTH`]). A present value must parse as an
/// integer and pass [`require_traversal_depth`]; a bad value fails open
/// naming the variable, the same contract as the memory limit. Pure in the
/// raw value, so tests never touch the process environment.
pub(crate) fn max_traversal_depth_from_setting(raw: Option<&str>) -> Result<u32> {
    match raw {
        Some(raw) => {
            let n = raw.trim().parse::<u32>().map_err(|_| {
                crate::Error::semantic(format!(
                    "invalid RELATE_MAX_TRAVERSAL_DEPTH: '{raw}' is not a number"
                ))
            })?;
            require_traversal_depth("RELATE_MAX_TRAVERSAL_DEPTH", n)
        }
        None => Ok(MAX_TRAVERSAL_DEPTH),
    }
}

/// The traversal-depth admission limit (LABS-610, ADR-0466): the flag's
/// `Some` value wins, then `RELATE_MAX_TRAVERSAL_DEPTH`, then the
/// default. A bad value fails the open before the store lease, the same
/// fail-before-lease contract as the memory limit.
pub(crate) fn resolve_max_traversal_depth(opts: &OpenOptions) -> Result<u32> {
    match opts.max_traversal_depth {
        Some(n) => require_traversal_depth("max_traversal_depth", n),
        None => {
            max_traversal_depth_from_setting(env_setting("RELATE_MAX_TRAVERSAL_DEPTH").as_deref())
        }
    }
}

/// Resolve the per-allocation expression cap (ADR-0463): an explicit
/// [`OpenOptions`] value wins; `None` reads
/// `RELATE_EXPRESSION_MEMORY_LIMIT` (`SIZE`, or `none`/`0` for no cap);
/// unset keeps the generous engine default — never silently off.
pub(crate) fn resolve_expression_memory_limit(opts: &OpenOptions) -> Result<Option<u64>> {
    let configured = match opts.expression_memory_limit {
        Some(n) => Some(n),
        None => expression_limit_from_setting(std::env::var("RELATE_EXPRESSION_MEMORY_LIMIT"))?,
    };
    Ok(match configured {
        // The SIZE grammar's `none`/`0` opt-out is `None` end to end.
        Some(0) => None,
        Some(n) => Some(n),
        None => Some(relate_executor::EXPRESSION_BUDGET_BYTES),
    })
}

/// Bytes from the raw `RELATE_EXPRESSION_MEMORY_LIMIT` read: `None`
/// (unset or blank) leaves the engine default to the caller; `none`/`0` is
/// the explicit no-cap opt-out. A present-but-unusable value (bad UTF-8 or a
/// bad size) fails open, like [`workspace_limit_from_setting`].
pub fn expression_limit_from_setting(
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<Option<u64>> {
    match present_env_value("RELATE_EXPRESSION_MEMORY_LIMIT", raw)? {
        Some(raw) => parse_expression_memory_limit(&raw)
            .map(Some)
            .map_err(|e| match e {
                crate::Error::Semantic(msg) => {
                    crate::Error::semantic(format!("invalid RELATE_EXPRESSION_MEMORY_LIMIT: {msg}"))
                }
                other => other,
            }),
        None => Ok(None),
    }
}

/// A positive u64 env knob with the memory limit's contract: a set value
/// that does not parse fails open with the same semantic error; zero is
/// rejected (these knobs gate on `>= threshold`, where 0 would batch
/// everything). Pure in the raw value, like [`memory_limit_from_setting`],
/// so tests never touch the process environment.
fn positive_u64_setting(name: &str, raw: Option<&str>, default: u64) -> Result<u64> {
    match raw {
        Some(raw) => raw
            .parse::<u64>()
            .ok()
            .filter(|v| *v > 0)
            .ok_or_else(|| crate::Error::semantic(format!("invalid {name} '{raw}'"))),
        None => Ok(default),
    }
}

/// The ADR-0209 bulk-write batching knobs, resolved once at open from
/// `RELATE_BULK_BATCH_THRESHOLD` / `RELATE_BULK_BATCH_BUDGET`
/// (tests set them to exercise batching on small graphs). Set-but-bad
/// values fail open, never silently fall back.
pub(crate) fn resolve_bulk_batch_limits() -> Result<(u64, u64)> {
    let threshold = positive_u64_setting(
        "RELATE_BULK_BATCH_THRESHOLD",
        env_setting("RELATE_BULK_BATCH_THRESHOLD").as_deref(),
        relate_executor::DEFAULT_BATCH_THRESHOLD,
    )?;
    let budget = positive_u64_setting(
        "RELATE_BULK_BATCH_BUDGET",
        env_setting("RELATE_BULK_BATCH_BUDGET").as_deref(),
        relate_executor::DEFAULT_BATCH_BUDGET,
    )?;
    Ok((threshold, budget))
}

/// A concurrent logical-workspace pool from an already-read
/// `RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT` value (ADR-0452).
///
/// Unset, blank, and whitespace leave admission off (`Ok(None)`). A
/// present value uses [`relate_core::parse_positive_memory_limit`],
/// the same `SIZE` grammar as the live-graph cap. `0`, a bad SIZE, and a
/// value that is not valid UTF-8 fail open naming the variable. This
/// takes the [`std::env::var`] result so a present-but-unusable value is
/// never treated as unset. Pure in that result, so tests never touch the
/// process environment.
pub fn workspace_limit_from_setting(
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<Option<u64>> {
    match present_env_value("RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT", raw)? {
        Some(raw) => {
            parse_positive_memory_limit("RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT", &raw).map(Some)
        }
        None => Ok(None),
    }
}

/// Token admission limits (ADR-0452). A positive concurrent
/// logical-workspace limit turns the pool on. Unset leaves it off. The
/// engine does not invent a default capacity.
pub(crate) fn resolve_token_limits(opts: &OpenOptions) -> Result<crate::token_budget::TokenLimits> {
    reject_renamed_env(
        "RELATE_CONCURRENT_PEAK_TOKEN_LIMIT",
        "RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT",
    )?;
    reject_renamed_env(
        "RELATE_TOKEN_ADMISSION",
        "RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT",
    )?;
    let capacity = match opts.concurrent_logical_workspace_limit {
        Some(n) => Some(require_positive_memory_limit(
            "concurrent_logical_workspace_limit",
            n,
        )?),
        None => workspace_limit_from_setting(std::env::var(
            "RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT",
        ))?,
    };
    Ok(crate::token_budget::TokenLimits { capacity })
}

/// The preview-feature opt-in from an already-read `RELATE_PREVIEW`
/// value (ADR-0371). Unset or empty enables nothing; an unknown feature
/// name fails open with a loud usage error naming the known names, the
/// same fail-closed contract as the bulk-batch knobs. A value present but
/// not valid UTF-8 also fails open naming the variable: the documented
/// contract is that an invalid value is never silently ignored, and
/// [`env_setting`] would fold `VarError::NotUnicode` into unset. Pure in
/// the raw value, so tests never touch the process environment.
pub(crate) fn preview_features_from_setting(
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<PreviewFeatures> {
    match present_env_value("RELATE_PREVIEW", raw)? {
        Some(value) => PreviewFeatures::parse(&value),
        // Unset and blank both enable nothing — `parse("")` is
        // `PreviewFeatures::none()`.
        None => Ok(PreviewFeatures::none()),
    }
}

/// The preview-feature opt-in (ADR-0371): `RELATE_PREVIEW`, read once
/// at open. See [`preview_features_from_setting`] for the contract.
pub(crate) fn resolve_preview_features() -> Result<PreviewFeatures> {
    preview_features_from_setting(std::env::var("RELATE_PREVIEW"))
}

/// The diagnostic pattern-engine switch (ADR-0519) from an already-read
/// `RELATE_PATTERN_ENGINE` value. Unset, blank, and whitespace are
/// [`PatternEngine::Auto`] (today's routing exactly); a present value
/// parses with [`PatternEngine::from_str`]'s grammar — `auto`, `nfa`, or
/// `walk` — and a value that is not one of the three fails open naming
/// the variable and all three, never a silent fallback. A value that is
/// not valid UTF-8 fails open naming the variable, the preview-feature
/// contract ([`present_env_value`]). Pure in the raw result, so tests
/// never touch the process environment.
pub(crate) fn pattern_engine_from_setting(
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<PatternEngine> {
    let Some(value) = present_env_value("RELATE_PATTERN_ENGINE", raw)? else {
        return Ok(PatternEngine::Auto);
    };
    value
        .parse()
        .map_err(|cause| Error::semantic(format!("invalid RELATE_PATTERN_ENGINE: {cause}")))
}

/// The diagnostic pattern-engine switch (ADR-0519):
/// `RELATE_PATTERN_ENGINE`, read once at open — the
/// `RELATE_PARSER` precedent's resolution site and precedence. See
/// [`pattern_engine_from_setting`] for the contract.
pub(crate) fn resolve_pattern_engine() -> Result<PatternEngine> {
    pattern_engine_from_setting(std::env::var("RELATE_PATTERN_ENGINE"))
}

pub(crate) fn resolve_checkpoint_interval(opts: &OpenOptions) -> Duration {
    match opts.checkpoint_interval {
        Some(d) => d,
        None => match env_setting("RELATE_CHECKPOINT_INTERVAL_SECS") {
            Some(s) => Duration::from_secs(s.parse().unwrap_or(DEFAULT_CHECKPOINT_INTERVAL_SECS)),
            None => Duration::from_secs(DEFAULT_CHECKPOINT_INTERVAL_SECS),
        },
    }
}

/// The commit durability policy (ADR-0077) from an already-read
/// `RELATE_DURABILITY_MODE` value (ADR-0470). Unset, blank, and
/// whitespace are `None` (the `OpenOptions` value, then the default
/// `full`, govern); a present value parses with
/// [`DurabilityMode::from_str`]'s grammar — a bad value fails with the
/// variable named, and a value that is not valid UTF-8 fails open naming
/// the variable, the preview-feature contract
/// ([`present_env_value`]). Pure in the raw result, so tests never touch
/// the process environment.
pub fn durability_mode_from_setting(
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<Option<DurabilityMode>> {
    let Some(value) = present_env_value("RELATE_DURABILITY_MODE", raw)? else {
        return Ok(None);
    };
    value
        .parse()
        .map(Some)
        .map_err(|cause| Error::semantic(format!("invalid RELATE_DURABILITY_MODE: {cause}")))
}

/// The commit durability policy (ADR-0077, ADR-0470): the option's
/// `Some(mode)` wins, then `RELATE_DURABILITY_MODE`, then
/// [`DurabilityMode::default`] (`full`) — the same ladder as
/// [`resolve_checkpoint_interval`], but with the fail-open contract every
/// value variable documented since the interval carries: a set value that
/// does not parse fails the open (in the fail-before-lease preflight),
/// never a silent default.
pub(crate) fn resolve_durability_mode(opts: &OpenOptions) -> Result<DurabilityMode> {
    Ok(match opts.durability_mode {
        Some(mode) => mode,
        None => durability_mode_from_setting(std::env::var("RELATE_DURABILITY_MODE"))?
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use relate_core::PreviewFeature;

    use super::*;
    use crate::{Error, OpenOptions};

    #[test]
    fn non_utf8_preview_setting_fails_open_naming_the_variable() {
        // The payload is never inspected: the resolver's contract keys on
        // the `VarError` variant, and `OsString::from_vec` (the realistic
        // invalid-bytes construction) does not exist on Windows. This
        // variant is exactly what `std::env::var` returns for a variable
        // whose bytes are not UTF-8.
        let err = preview_features_from_setting(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("not utf-8"),
        )))
        .unwrap_err();
        assert!(matches!(err, Error::Semantic(_)), "{err}");
        let display = err.to_string();
        assert!(display.contains("RELATE_PREVIEW"), "{display}");
        assert!(display.contains("not valid UTF-8"), "{display}");
    }

    #[test]
    fn preview_setting_keeps_unset_empty_and_unknown_contract() {
        assert_eq!(
            preview_features_from_setting(Err(std::env::VarError::NotPresent)).unwrap(),
            PreviewFeatures::none(),
        );
        assert_eq!(
            preview_features_from_setting(Ok(String::new())).unwrap(),
            PreviewFeatures::none(),
        );
        assert_eq!(
            preview_features_from_setting(Ok("   ".to_string())).unwrap(),
            PreviewFeatures::none(),
        );
        let enabled = preview_features_from_setting(Ok("ontology".to_string())).unwrap();
        assert!(enabled.enabled(PreviewFeature::Ontology));
        let err = preview_features_from_setting(Ok("bogus".to_string())).unwrap_err();
        assert!(
            err.to_string().contains("unknown preview feature 'bogus'"),
            "{err}"
        );
    }

    #[test]
    fn markdown_bodies_setting_parses_unset_and_fails_open() {
        // The env contract of the sibling resolvers: unset/blank is None
        // (the sidecar governs), a good spec parses with the flag
        // grammar, a bad spec is its parse error, and non-UTF-8 fails
        // open naming the variable.
        assert_eq!(
            markdown_bodies_from_setting(Err(std::env::VarError::NotPresent)).unwrap(),
            None
        );
        assert_eq!(
            markdown_bodies_from_setting(Ok(String::new())).unwrap(),
            None
        );
        assert_eq!(
            markdown_bodies_from_setting(Ok("   ".to_string())).unwrap(),
            None
        );
        let config = markdown_bodies_from_setting(Ok("summary,notes".to_string()))
            .unwrap()
            .expect("a good spec parses");
        assert_eq!(config.default, vec!["summary", "notes"]);
        let bad = markdown_bodies_from_setting(Ok("summary,,notes".to_string())).unwrap_err();
        assert!(
            bad.to_string().contains("an entry names no property"),
            "{bad}"
        );
        let utf8 = markdown_bodies_from_setting(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("not utf-8"),
        )))
        .unwrap_err();
        assert!(
            utf8.to_string()
                .contains("RELATE_MARKDOWN_BODY_PROPERTIES: value is not valid UTF-8"),
            "{utf8}"
        );
    }

    #[test]
    fn bad_bulk_batch_setting_fails_open() {
        let err = positive_u64_setting("RELATE_BULK_BATCH_THRESHOLD", Some("1tb"), 7).unwrap_err();
        assert!(
            err.to_string().contains("RELATE_BULK_BATCH_THRESHOLD"),
            "{err}"
        );
        let err = positive_u64_setting("RELATE_BULK_BATCH_BUDGET", Some("0"), 7).unwrap_err();
        assert!(
            err.to_string().contains("RELATE_BULK_BATCH_BUDGET"),
            "{err}"
        );
        assert_eq!(positive_u64_setting("X", None, 7).unwrap(), 7);
        assert_eq!(positive_u64_setting("X", Some("3"), 7).unwrap(), 3);
    }

    #[test]
    fn bad_memory_limit_setting_is_the_parse_error() {
        let err = memory_limit_from_setting(Some("1tb")).unwrap_err();
        assert_eq!(
            err,
            Error::semantic("invalid memory limit unit 'tb'; use bytes, mb, mib, gb, or gib")
        );
        assert_eq!(memory_limit_from_setting(None).unwrap(), 0);
        let ignored = renamed_env_error(
            "RELATE_MEMORY_LIMIT",
            "RELATE_LOGICAL_GRAPH_SIZE_LIMIT",
            Ok("1mib".into()),
        )
        .unwrap_err();
        assert!(
            ignored
                .to_string()
                .contains("RELATE_MEMORY_LIMIT is set but ignored"),
            "{ignored}"
        );
        assert!(renamed_env_error(
            "RELATE_MEMORY_LIMIT",
            "RELATE_LOGICAL_GRAPH_SIZE_LIMIT",
            Err(std::env::VarError::NotPresent),
        )
        .is_ok());
        assert_eq!(memory_limit_from_setting(Some("2mb")).unwrap(), 2_000_000);
        // An explicit OpenOptions value wins; the env string is not consulted.
        let opts = OpenOptions {
            logical_graph_size_limit: Some(1),
            ..Default::default()
        };
        assert_eq!(resolve_memory_limit(&opts).unwrap(), 1);
    }

    #[test]
    fn expression_limit_resolution_has_a_generous_default_and_an_opt_out() {
        // Unset (or blank) leaves the engine default to the resolver.
        assert_eq!(
            expression_limit_from_setting(Err(std::env::VarError::NotPresent)).unwrap(),
            None
        );
        assert_eq!(
            expression_limit_from_setting(Ok(String::new())).unwrap(),
            None
        );
        // The default is the measured 256 MiB.
        assert_eq!(relate_executor::EXPRESSION_BUDGET_BYTES, 256 << 20);
        // Explicit values: a raise, and the spelled/zero opt-out.
        assert_eq!(
            expression_limit_from_setting(Ok("512mib".into())).unwrap(),
            Some(512 << 20)
        );
        assert_eq!(
            expression_limit_from_setting(Ok("none".into())).unwrap(),
            Some(0)
        );
        assert_eq!(
            expression_limit_from_setting(Ok("NONE".into())).unwrap(),
            Some(0)
        );
        assert_eq!(
            expression_limit_from_setting(Ok("0".into())).unwrap(),
            Some(0)
        );
        // A bad value is the shared parse error; a non-UTF-8 value fails
        // open rather than silently reading as unset.
        let err = expression_limit_from_setting(Ok("1tb".into())).unwrap_err();
        assert!(
            err.to_string()
                .contains("invalid RELATE_EXPRESSION_MEMORY_LIMIT:"),
            "{err}"
        );
        assert!(
            expression_limit_from_setting(Err(std::env::VarError::NotUnicode(
                std::ffi::OsString::from("x")
            )))
            .is_err()
        );
        // An explicit OpenOptions value wins over the env/default.
        let opts = OpenOptions {
            expression_memory_limit: Some(42),
            ..Default::default()
        };
        assert_eq!(resolve_expression_memory_limit(&opts).unwrap(), Some(42));
        let opts = OpenOptions {
            expression_memory_limit: Some(0),
            ..Default::default()
        };
        assert_eq!(resolve_expression_memory_limit(&opts).unwrap(), None);
    }

    #[test]
    fn workspace_limit_setting_enables_only_when_positive() {
        assert_eq!(
            workspace_limit_from_setting(Err(std::env::VarError::NotPresent)).unwrap(),
            None
        );
        assert_eq!(
            workspace_limit_from_setting(Ok(String::new())).unwrap(),
            None
        );
        assert_eq!(
            workspace_limit_from_setting(Ok("   ".into())).unwrap(),
            None
        );
        assert_eq!(
            workspace_limit_from_setting(Ok("4096".into())).unwrap(),
            Some(4096)
        );
        assert_eq!(
            workspace_limit_from_setting(Ok("2mib".into())).unwrap(),
            Some(2 * 1024 * 1024)
        );
        let zero = workspace_limit_from_setting(Ok("0".into())).unwrap_err();
        assert!(
            zero.to_string()
                .contains("invalid RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT: must be positive"),
            "{zero}"
        );
        let bad = workspace_limit_from_setting(Ok("1tb".into())).unwrap_err();
        assert!(
            bad.to_string()
                .contains("invalid RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT:"),
            "{bad}"
        );
        let utf8 = workspace_limit_from_setting(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("not utf-8"),
        )))
        .unwrap_err();
        assert!(
            utf8.to_string().contains(
                "invalid RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT: value is not valid UTF-8"
            ),
            "{utf8}"
        );
        let ignored = renamed_env_error(
            "RELATE_TOKEN_ADMISSION",
            "RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT",
            Ok("1".into()),
        )
        .unwrap_err();
        assert!(
            ignored
                .to_string()
                .contains("RELATE_TOKEN_ADMISSION is set but ignored"),
            "{ignored}"
        );
        let on = resolve_token_limits(&OpenOptions {
            concurrent_logical_workspace_limit: Some(8192),
            ..OpenOptions::default()
        })
        .unwrap();
        assert_eq!(on.capacity, Some(8192));
        let zero_option = resolve_token_limits(&OpenOptions {
            concurrent_logical_workspace_limit: Some(0),
            ..OpenOptions::default()
        })
        .unwrap_err();
        assert!(
            zero_option
                .to_string()
                .contains("invalid concurrent_logical_workspace_limit: must be positive"),
            "{zero_option}"
        );
    }

    /// LABS-610: the traversal-depth knob resolves flag first, then the
    /// environment, then 500; 0 and anything over the 2000 ceiling fail.
    #[test]
    fn traversal_depth_resolves_default_flag_and_env_contract() {
        assert_eq!(
            max_traversal_depth_from_setting(None).unwrap(),
            relate_core::MAX_TRAVERSAL_DEPTH
        );
        assert_eq!(
            max_traversal_depth_from_setting(Some("2000")).unwrap(),
            2000
        );
        for bad in ["0", "2001", "nope"] {
            let err = max_traversal_depth_from_setting(Some(bad)).unwrap_err();
            assert!(
                err.to_string().contains("RELATE_MAX_TRAVERSAL_DEPTH"),
                "{bad}: {err}"
            );
        }
        let explicit = resolve_max_traversal_depth(&OpenOptions {
            max_traversal_depth: Some(1200),
            ..OpenOptions::default()
        })
        .unwrap();
        assert_eq!(explicit, 1200);
        let zero = resolve_max_traversal_depth(&OpenOptions {
            max_traversal_depth: Some(0),
            ..OpenOptions::default()
        })
        .unwrap_err();
        assert!(
            zero.to_string()
                .contains("invalid max_traversal_depth: must be between 1 and 2000"),
            "{zero}"
        );
    }

    /// ADR-0470: the durability mode's setting contract — unset/blank is
    /// `None`, each mode name parses (case-insensitively, the
    /// `DurabilityMode::from_str` grammar), a bad value fails naming the
    /// variable and the mode vocabulary, and non-UTF-8 fails open naming
    /// the variable.
    #[test]
    fn durability_mode_setting_parses_unset_and_fails_open() {
        use crate::DurabilityMode;
        assert_eq!(
            durability_mode_from_setting(Err(std::env::VarError::NotPresent)).unwrap(),
            None
        );
        assert_eq!(
            durability_mode_from_setting(Ok(String::new())).unwrap(),
            None
        );
        assert_eq!(
            durability_mode_from_setting(Ok("   ".to_string())).unwrap(),
            None
        );
        // The shared table names the mode through the `relate` the
        // test-support crate links — a distinct crate instance from this
        // unit-test build's — so the pin compares the derived `Debug`
        // (the variant name), independent of the Display/FromStr
        // spellings under test.
        for &(raw, mode) in relate_test_support::durability::DURABILITY_MODES {
            let parsed = durability_mode_from_setting(Ok(raw.to_string()))
                .unwrap()
                .expect("a present mode name parses");
            assert_eq!(format!("{parsed:?}"), format!("{mode:?}"), "{raw}");
        }
        // Case-insensitive, the `from_str` grammar.
        assert_eq!(
            durability_mode_from_setting(Ok("Snapshot".to_string())).unwrap(),
            Some(DurabilityMode::Snapshot)
        );
        let bad = durability_mode_from_setting(Ok("everysecnd".to_string())).unwrap_err();
        let display = bad.to_string();
        assert!(display.contains("RELATE_DURABILITY_MODE"), "{display}");
        assert!(
            display.contains("expected full, everysecond, or snapshot"),
            "{display}"
        );
        let utf8 = durability_mode_from_setting(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("not utf-8"),
        )))
        .unwrap_err();
        assert!(
            utf8.to_string()
                .contains("RELATE_DURABILITY_MODE: value is not valid UTF-8"),
            "{utf8}"
        );
    }

    /// ADR-0470: the resolution ladder — the option's `Some(mode)` wins
    /// (the environment is not consulted for it), and the default option
    /// resolves to the engine default when the variable is unset in this
    /// process (the variable's own parse contract is pinned above, pure
    /// in the raw value).
    #[test]
    fn durability_mode_resolves_flag_first_then_env_then_default() {
        use crate::DurabilityMode;
        for &(raw, _) in relate_test_support::durability::DURABILITY_MODES {
            let mode: DurabilityMode = raw.parse().expect("a mode name parses");
            assert_eq!(
                resolve_durability_mode(&OpenOptions {
                    durability_mode: Some(mode),
                    ..OpenOptions::default()
                })
                .unwrap(),
                mode
            );
        }
        assert_eq!(
            resolve_durability_mode(&OpenOptions::default()).unwrap(),
            DurabilityMode::Full
        );
    }

    /// ADR-0519: the diagnostic pattern-engine switch's setting contract
    /// — unset/blank/`auto` is today's routing exactly, `nfa` and `walk`
    /// parse, a bad value fails open naming the variable and the three
    /// valid values, and non-UTF-8 fails open naming the variable.
    #[test]
    fn pattern_engine_setting_parses_unset_and_fails_open() {
        assert_eq!(
            pattern_engine_from_setting(Err(std::env::VarError::NotPresent)).unwrap(),
            PatternEngine::Auto
        );
        assert_eq!(
            pattern_engine_from_setting(Ok(String::new())).unwrap(),
            PatternEngine::Auto
        );
        assert_eq!(
            pattern_engine_from_setting(Ok("   ".to_string())).unwrap(),
            PatternEngine::Auto
        );
        assert_eq!(
            pattern_engine_from_setting(Ok("auto".to_string())).unwrap(),
            PatternEngine::Auto
        );
        assert_eq!(
            pattern_engine_from_setting(Ok("nfa".to_string())).unwrap(),
            PatternEngine::Nfa
        );
        assert_eq!(
            pattern_engine_from_setting(Ok("walk".to_string())).unwrap(),
            PatternEngine::Walk
        );
        let bad = pattern_engine_from_setting(Ok("dfs".to_string())).unwrap_err();
        let display = bad.to_string();
        assert!(display.contains("RELATE_PATTERN_ENGINE"), "{display}");
        assert!(display.contains("expected auto, nfa, or walk"), "{display}");
        let utf8 = pattern_engine_from_setting(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("not utf-8"),
        )))
        .unwrap_err();
        assert!(
            utf8.to_string()
                .contains("RELATE_PATTERN_ENGINE: value is not valid UTF-8"),
            "{utf8}"
        );
    }
}

#[cfg(test)]
mod cypherlite_env_guard_tests {
    use super::current_spelling;

    #[test]
    fn double_renamed_knobs_name_their_current_spelling() {
        assert_eq!(
            current_spelling("CYPHERLITE_MEMORY_LIMIT"),
            Some("RELATE_LOGICAL_GRAPH_SIZE_LIMIT")
        );
        assert_eq!(
            current_spelling("CYPHERLITE_TOKEN_ADMISSION"),
            Some("RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT")
        );
        assert_eq!(
            current_spelling("CYPHERLITE_CONCURRENT_PEAK_TOKEN_LIMIT"),
            Some("RELATE_CONCURRENT_LOGICAL_WORKSPACE_LIMIT")
        );
        assert_eq!(current_spelling("CYPHERLITE_PREVIEW"), None);
    }
}
