//! "Did you mean" matching for unknown flags (ADR-0192) and, as a fallback
//! after a failed parse, for mistyped command words (ADR-0230): a pure
//! string helper. The candidate lists live with the help data in
//! [`crate::help`], and [`crate::parse_error::ParseError::suggestion`],
//! [`crate::errors::report_query_error`], and the query-run flag parser call
//! in here.

use crate::help::{commands, CommandSpec};

/// The command a failed statement's first word was plausibly a typo of
/// (ADR-0230). Runs strictly as a fallback after the engine rejects the
/// statement, so parsing stays authoritative and valid Cypher with a
/// command-like first token is untouched. Candidates come from
/// [`commands()`] itself — every compiled-in command word plus its declared
/// aliases (`update` for `upgrade`) — so the suggestion set cannot drift
/// from `--help`.
pub(crate) fn suggest_command(statement: &str) -> Option<&'static CommandSpec> {
    // The first word-like token; a statement that opens with anything else
    // (a digit, a parenthesis, a quote) is not a mistyped command.
    let needle = fold_word(first_word(statement)?);
    if needle.len() < 2 {
        return None;
    }
    let limit = near_miss_limit(needle.len());
    commands()
        .iter()
        .flat_map(|spec| {
            std::iter::once(spec.word)
                .chain(spec.aliases.iter().copied())
                .map(move |w| (w, spec))
        })
        .filter_map(|(word, spec)| {
            let candidate = fold_word(word);
            let distance = edit_distance(&needle, &candidate);
            let prefix = candidate.starts_with(&needle);
            // A prefix match wins a distance tie: "qu" is `query` cut short,
            // not one edit away from something else.
            let rank = if prefix { 0 } else { 1 };
            (distance <= limit || prefix).then_some((distance, rank, spec))
        })
        .min_by_key(|&(distance, rank, _)| (distance, rank))
        .map(|(_, _, spec)| spec)
}

/// The rendered ADR-0230 suggestion line for a failed statement, if its
/// first word is plausibly a mistyped command: `did you mean "query"?  --`
/// plus the command's one-line synopsis. Owns the engine-path applicability
/// rule — syntax errors only — beside the matcher, so the error-reporting
/// module decides and formats nothing; the line itself is
/// [`command_line`], the one renderer, shared with the CLI's usage-error
/// exits (ADR-0518).
pub(crate) fn command_suggestion_line(e: &relate::Error, statement: &str) -> Option<String> {
    if !matches!(e, relate::Error::Syntax(_)) {
        return None;
    }
    suggest_command(statement).map(command_line)
}

/// The suggestion line for one matched command: `did you mean "query"?  --`
/// plus the command's one-line synopsis, first letter lowered. The one
/// renderer every suggestion path prints — the engine-error fallback above
/// and the usage-error exits (ADR-0518), so the copies cannot drift.
pub(crate) fn command_line(spec: &CommandSpec) -> String {
    format!(
        "did you mean \"{}\"?  -- {}",
        spec.word,
        lowercase_first(spec.blurb)
    )
}

/// `s` with its first character lowercased (Unicode-aware, never
/// byte-indexed: a non-ASCII blurb cannot panic the error path).
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The first token of the statement when it starts with a word:
/// `[A-Za-z][A-Za-z0-9_-]*` immediately after leading whitespace. A
/// statement opening with anything else (a digit, a parenthesis, a quote) is
/// not a mistyped command and earns no suggestion.
fn first_word(statement: &str) -> Option<&str> {
    let rest = statement.trim_start();
    if !rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The one fold both matchers use, for a bare word: dashes and underscores
/// ignored, case folded.
fn fold_word(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| !matches!(c, '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The closest of `candidates` to the unknown flag `unknown`, if any is near
/// enough to be a plausible typo. Dashes and underscores are ignored and case
/// is folded, so `--readonly` and `--read_only` match `--read-only`. An
/// argument that does not start with `-` is not a flag and gets no suggestion.
/// `aliases` are exact-match suggestions for spellings the edit distance
/// cannot reach (ADR-0239); the table lives with the flag data in
/// [`crate::help`], passed in like `candidates`, so this stays a pure string
/// helper with no flag knowledge of its own. The candidates borrow from the
/// caller's list — a static flag table or an extension manifest's flag
/// entries — and the suggestion borrows with them, so it is candidate text
/// itself, never a copy the caller must keep in sync.
pub(crate) fn suggest_flag<'a>(
    unknown: &str,
    candidates: &[&'a str],
    aliases: &[(&str, &'a str)],
) -> Option<&'a str> {
    if !unknown.starts_with('-') {
        return None;
    }
    // Fixed aliases (ADR-0239): spellings from other CLIs that the
    // edit-distance matcher cannot bridge to the real flag. The unknown
    // flag stays an error; only the suggestion names the right spelling.
    // Like the edit-distance arm, the target must be a flag this caller's
    // parser actually accepts — the alias table is the top-level one, so a
    // subcommand candidate list without the target yields no suggestion.
    for &(alias, target) in aliases {
        if unknown.eq_ignore_ascii_case(alias) && candidates.contains(&target) {
            return Some(target);
        }
    }
    let needle = fold_word(unknown.trim_start_matches('-'));
    if needle.is_empty() {
        return None;
    }
    let limit = near_miss_limit(needle.len());
    candidates
        .iter()
        .map(|&c| (edit_distance(&needle, &fold_word(c)), c))
        .filter(|&(d, _)| d <= limit)
        .min_by_key(|&(d, _)| d)
        .map(|(_, c)| c)
}

/// How far a near-miss may drift and still be a plausible typo: a third of
/// its length, at least one edit. Both matchers share the one knob.
fn near_miss_limit(len: usize) -> usize {
    (len / 3).max(1)
}

/// Levenshtein distance between two char slices.
fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::help::{session_flags, FLAG_ALIASES};

    #[test]
    fn suggest_command_finds_near_misses_of_command_words() {
        let word = |s: &str| suggest_command(s).map(|spec| spec.word);
        // Prefixes and one-edit typos of compiled-in words.
        assert_eq!(word("qu"), Some("query"));
        assert_eq!(word("clon ./a ./b"), Some("clone"));
        // Compiled-out commands are not candidates (ADR-0104).
        #[cfg(feature = "telemetry")]
        assert_eq!(word("telemetr status"), Some("telemetry"));
        // The `update` alias reports the canonical word.
        #[cfg(feature = "upgrade")]
        assert_eq!(word("updat --check"), Some("upgrade"));
        // Leading whitespace and dashes/underscores fold away.
        assert_eq!(word("  que-ry \"RETURN 1\""), Some("query"));
    }

    #[test]
    fn suggest_command_leaves_genuine_parse_errors_alone() {
        // Cypher keywords, far words, one-letter tokens, and a non-word
        // opening are not mistyped commands.
        for statement in [
            "MATCH (n RETURN n",
            "RETURN 1 +",
            "CREATE (",
            "frobnicate everything",
            "q",
            "(n)",
            "(qu)",
            "42clon",
            r#""qu""#,
            "",
        ] {
            assert!(suggest_command(statement).is_none(), "{statement:?}");
        }
    }

    #[test]
    fn suggest_flag_finds_near_misses_only() {
        let s = |u| suggest_flag(u, session_flags(), FLAG_ALIASES);
        assert_eq!(s("--readonly"), Some("--read-only"));
        assert_eq!(s("--read_only"), Some("--read-only"));
        assert_eq!(s("--jsn"), Some("--json"));
        assert_eq!(s("--bogus"), None);
        assert_eq!(s("--"), None);
        // Not a flag: no suggestion, however close the spelling.
        assert_eq!(s("json"), None);
        assert_eq!(s("read-only"), None);
    }

    #[test]
    fn suggest_flag_maps_other_cli_spellings_to_format() {
        let s = |u| suggest_flag(u, session_flags(), FLAG_ALIASES);
        // ADR-0239: `--output`/`-o` (neo4j-cli) point at `--format`; the
        // suggestion names a flag the parser actually accepts.
        assert_eq!(s("--output"), Some("--format"));
        assert_eq!(s("-o"), Some("--format"));
        assert!(session_flags().contains(&"--format"));
        // Without the alias table the helper is pure edit distance.
        assert_eq!(suggest_flag("--output", session_flags(), &[]), None);
        // A caller whose parser does not accept the alias target gets no
        // suggestion: a subcommand never names a flag it would reject.
        assert_eq!(
            suggest_flag("--output", &["--agent", "--force"], FLAG_ALIASES),
            None
        );
    }
}
