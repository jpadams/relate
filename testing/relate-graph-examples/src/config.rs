//! Shared env-knob policy. Every env-var → parse → default chain in this
//! crate routes through here, so the unset-vs-invalid distinction and the
//! boolean-flag shape live in exactly one place instead of being
//! hand-rolled per call site.

use crate::manifest::{self, Dataset};

/// Read a positive-integer knob from env var `name`.
///
/// Unset falls back to `default` silently. A present-but-invalid value
/// (not a `u64`, or zero) warns naming the var and the bad value, then
/// falls back to `default`: a typo'd knob must be loud, never silently
/// ignored.
pub fn env_or(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Err(_) => default,
        Ok(v) => match v.parse::<u64>() {
            Ok(n) if n > 0 => n,
            _ => {
                eprintln!(
                    "warning: {name}={v:?} is not a positive integer; using default {default}"
                );
                default
            }
        },
    }
}

/// Read a boolean env flag: true exactly when the var is set to `1`.
pub fn env_flag(name: &str) -> bool {
    std::env::var(name).ok().as_deref() == Some("1")
}

/// Parse a dataset-filter value (read from env var `name`, used in the
/// error message) into the selected datasets, in manifest order.
/// `flag_enabled` reads one env flag (given its name) — the hook
/// [`dataset_filter`] passes the real env; unit tests pass a closure so
/// the rule stays testable without process-global env mutation.
///
/// `None` (unset) or an empty/whitespace-only value selects every
/// dataset in the manifest **except** the opt-in ones (a heavy dataset
/// delays the whole suite, so it runs only on its explicit flag —
/// `RELATE_GRAPH_EXAMPLES_TWITCH=1` for twitch — or by being named
/// here). Names are comma-separated and trimmed; a name absent from the
/// manifest is an error naming the value and listing the valid names —
/// a typo'd dataset must fail loudly before any docker work, never
/// silently select nothing. Naming an opt-in dataset here always runs
/// it: the explicit list is an explicit opt-in.
pub fn parse_dataset_filter(
    name: &str,
    value: Option<&str>,
    flag_enabled: impl Fn(&str) -> bool,
) -> Result<Vec<&'static Dataset>, String> {
    let names: Vec<&str> = value
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    let explicit = !names.is_empty();
    if explicit {
        for &n in &names {
            if Dataset::find(n).is_none() {
                let valid = manifest::DATASETS
                    .iter()
                    .map(|d| d.name)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "{name}: unknown dataset {n:?}; valid names: {valid}"
                ));
            }
        }
    }
    // Manifest order, not filter order: the run's wave shape and report
    // stay stable however the names were listed. An opt-in dataset is
    // selected only when its flag is `1` or it was named explicitly.
    Ok(manifest::DATASETS
        .iter()
        .filter(|d| match d.opt_in_env {
            Some(flag) if !explicit => flag_enabled(flag),
            _ => !explicit || names.contains(&d.name),
        })
        .collect())
}

/// Read the dataset filter from env var `name`; see
/// [`parse_dataset_filter`]. A non-UTF-8 value cannot be interpreted, so
/// it warns naming the var (the value itself cannot be printed) and falls
/// back to every dataset — the same loud-fallback policy as [`env_or`].
pub fn dataset_filter(name: &str) -> Result<Vec<&'static Dataset>, String> {
    // The real env hook for `parse_dataset_filter`'s flag predicate.
    let flag_enabled = |flag: &str| std::env::var(flag).ok().as_deref() == Some("1");
    match std::env::var(name) {
        Ok(v) => parse_dataset_filter(name, Some(&v), flag_enabled),
        Err(std::env::VarError::NotPresent) => parse_dataset_filter(name, None, flag_enabled),
        Err(std::env::VarError::NotUnicode(_)) => {
            eprintln!("warning: {name} is not valid UTF-8; using all datasets");
            parse_dataset_filter(name, None, flag_enabled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Env is process-global, so each test uses its own var name: parallel
    // tests must never share a knob.
    #[test]
    fn env_or_unset_returns_the_default() {
        assert_eq!(env_or("RELATE_GRAPH_EXAMPLES_TEST_UNSET", 7), 7);
    }

    #[test]
    fn env_or_parses_a_present_value() {
        std::env::set_var("RELATE_GRAPH_EXAMPLES_TEST_VALID", "42");
        assert_eq!(env_or("RELATE_GRAPH_EXAMPLES_TEST_VALID", 7), 42);
        std::env::remove_var("RELATE_GRAPH_EXAMPLES_TEST_VALID");
    }

    #[test]
    fn env_or_garbage_falls_back_to_the_default() {
        std::env::set_var("RELATE_GRAPH_EXAMPLES_TEST_GARBAGE", "abc");
        assert_eq!(env_or("RELATE_GRAPH_EXAMPLES_TEST_GARBAGE", 7), 7);
        std::env::remove_var("RELATE_GRAPH_EXAMPLES_TEST_GARBAGE");
    }

    #[test]
    fn env_or_zero_is_invalid() {
        std::env::set_var("RELATE_GRAPH_EXAMPLES_TEST_ZERO", "0");
        assert_eq!(env_or("RELATE_GRAPH_EXAMPLES_TEST_ZERO", 7), 7);
        std::env::remove_var("RELATE_GRAPH_EXAMPLES_TEST_ZERO");
    }

    #[test]
    fn env_flag_is_true_exactly_on_1() {
        const NAME: &str = "RELATE_GRAPH_EXAMPLES_TEST_FLAG";
        assert!(!env_flag(NAME));
        std::env::set_var(NAME, "0");
        assert!(!env_flag(NAME));
        std::env::set_var(NAME, "yes");
        assert!(!env_flag(NAME));
        std::env::set_var(NAME, "1");
        assert!(env_flag(NAME));
        std::env::remove_var(NAME);
    }

    fn selected_names(value: Option<&str>) -> Vec<&'static str> {
        selected_names_with_flags(value, |_| false)
    }

    fn selected_names_with_flags(
        value: Option<&str>,
        flag_enabled: impl Fn(&str) -> bool,
    ) -> Vec<&'static str> {
        parse_dataset_filter("TEST_DATASETS", value, flag_enabled)
            .unwrap()
            .iter()
            .map(|d| d.name)
            .collect()
    }

    #[test]
    fn dataset_filter_unset_selects_every_dataset_except_opt_ins() {
        let expected: Vec<&str> = manifest::DATASETS
            .iter()
            .filter(|d| d.opt_in_env.is_none())
            .map(|d| d.name)
            .collect();
        assert_eq!(selected_names(None), expected);
        assert_eq!(selected_names(Some("")), expected);
        assert_eq!(selected_names(Some("  ")), expected);
    }

    /// An opt-in dataset's flag turns it back on for the default run.
    #[test]
    fn dataset_filter_opt_in_flag_reenables_the_opt_in_dataset() {
        let names = selected_names_with_flags(None, |flag| flag == "RELATE_GRAPH_EXAMPLES_TWITCH");
        assert!(
            names.contains(&"twitch"),
            "flag re-enables twitch: {names:?}"
        );
    }

    /// Naming an opt-in dataset in the filter always runs it — the
    /// explicit list is an explicit opt-in.
    #[test]
    fn dataset_filter_explicit_name_runs_the_opt_in_dataset() {
        assert_eq!(selected_names(Some("twitch, movies")), ["movies", "twitch"]);
    }

    #[test]
    fn dataset_filter_selects_a_subset() {
        assert_eq!(
            selected_names(Some("fraud-detection,pole")),
            ["fraud-detection", "pole"]
        );
    }

    #[test]
    fn dataset_filter_trims_and_returns_manifest_order() {
        assert_eq!(selected_names(Some(" pole , movies ")), ["movies", "pole"]);
    }

    #[test]
    fn dataset_filter_unknown_name_is_an_error() {
        let err = parse_dataset_filter("TEST_DATASETS", Some("nope"), |_| false).unwrap_err();
        assert!(err.contains("nope"), "error names the value: {err}");
        assert!(
            err.contains("movies") && err.contains("fincen"),
            "error lists the valid names: {err}"
        );
    }

    #[test]
    fn dataset_filter_reads_the_env_var() {
        const NAME: &str = "RELATE_GRAPH_EXAMPLES_TEST_DATASETS";
        std::env::set_var(NAME, "pole");
        let names: Vec<&str> = dataset_filter(NAME)
            .unwrap()
            .iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(names, ["pole"]);
        std::env::remove_var(NAME);
    }

    /// A non-UTF-8 value is unreadable, so the filter warns naming the var
    /// and falls back to every dataset (mirroring `env_or`'s loud fallback).
    /// Unix-only: building an `OsStr` from arbitrary bytes is a unix API.
    #[cfg(unix)]
    #[test]
    fn dataset_filter_non_unicode_value_falls_back_to_all_datasets() {
        use std::os::unix::ffi::OsStrExt;

        const NAME: &str = "RELATE_GRAPH_EXAMPLES_TEST_NON_UNICODE";
        std::env::set_var(NAME, std::ffi::OsStr::from_bytes(b"\xff\xfe"));
        let selected = dataset_filter(NAME).unwrap();
        assert_eq!(selected.len(), manifest::DATASETS.len() - 1); // twitch stays opt-in
        std::env::remove_var(NAME);
    }
}
