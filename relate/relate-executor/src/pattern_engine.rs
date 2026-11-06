//! The diagnostic pattern-engine switch (ADR-0519): the typed value the
//! kernels' routing consults, never re-derived at a rung. Resolved once
//! at `Database::open` from `RELATE_PATTERN_ENGINE` (the
//! `RELATE_PARSER` precedent's resolution site and precedence,
//! ADR-0277's history) and installed on the store-level service bundle
//! (`Graph::configure_pattern_engine` in `graph/services`); the rungs
//! read it back through `Graph::pattern_engine`.

use std::str::FromStr;

use relate_core::{Error, Result};

/// Which pattern engine serves a clause (ADR-0519): a diagnostic-only
/// switch for the pattern-engine spike — never Cypher syntax, no
/// query-level spelling, and no stability promise. It dies with the old
/// engines it switches between.
///
/// Read once at `Database::open` from `RELATE_PATTERN_ENGINE`:
/// unset, blank, and `auto` are [`PatternEngine::Auto`] (today's routing
/// exactly); `nfa` forces the frontier family's rung (ADR-0478) onto
/// every frontier-compilable clause, bypassing that rung's cost gate —
/// every structural, answer-shape, allocation and budget proof stands,
/// and a decline still fails through; `walk` stands the traversal
/// kernels down (the `with_kernels_disabled` differential behavior,
/// env-reachable), so the batch scan-filter chain and the per-row walker
/// serve every clause.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PatternEngine {
    /// Today's routing exactly: every rung's own proofs and cost gates
    /// decide, unchanged.
    #[default]
    Auto,
    /// Force the frontier family's rung for every frontier-compilable
    /// clause (the cost gate `frontier_match::frontier_engages` is
    /// bypassed); the batch/chain-kernel rungs stand down for the
    /// clauses the frontier takes — they sit below it on both operator
    /// ladders and decline the quantified shapes structurally.
    Nfa,
    /// Stand the traversal kernels down: the chain/hub-star kernels
    /// (ADR-0473) and the frontier family (ADR-0478) decline every
    /// clause, and the batch scan-filter chain plus the per-row walker
    /// serve — the differential suites' `with_kernels_disabled`
    /// behavior, reachable from a real process.
    Walk,
}

impl FromStr for PatternEngine {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "nfa" => Ok(Self::Nfa),
            "walk" => Ok(Self::Walk),
            other => Err(Error::semantic(format!(
                "invalid pattern engine {other:?}; expected auto, nfa, or walk"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The value vocabulary, one parse each: the three valid values
    /// (case-insensitively, the `DurabilityMode` grammar) and the
    /// unknown value's authored rejection naming all three.
    #[test]
    fn parses_the_three_values_and_rejects_the_rest() {
        assert_eq!(
            "auto".parse::<PatternEngine>().unwrap(),
            PatternEngine::Auto
        );
        assert_eq!("nfa".parse::<PatternEngine>().unwrap(), PatternEngine::Nfa);
        assert_eq!(
            "walk".parse::<PatternEngine>().unwrap(),
            PatternEngine::Walk
        );
        assert_eq!(
            " NFA ".parse::<PatternEngine>().unwrap(),
            PatternEngine::Nfa,
            "trimmed, case-insensitive"
        );
        let err = "dfs".parse::<PatternEngine>().unwrap_err();
        let display = err.to_string();
        assert!(display.contains("invalid pattern engine"), "{display}");
        assert!(display.contains("expected auto, nfa, or walk"), "{display}");
    }
}
