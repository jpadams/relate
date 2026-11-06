//! Integer-literal parsing: the one decision point for every consumer of an
//! `Integer` token (`parse_primary`, the hop/quantifier bound, the vector
//! dimension), so a removed Cypher 25 spelling cannot slip through a
//! non-primary site.
//!
//! Cypher 25 keeps the lowercase `0x`/`0o` prefixed forms and rejects the
//! uppercase prefixes plus the legacy octal-looking leading-zero decimal
//! (`01277`, `00`) — live-verified against neo4j:2026.09-enterprise. A lone
//! `0` is an ordinary decimal literal.

use super::*;

/// Why an integer token is not a value: its digits do not fit `i64`.
///
/// The removed spellings (uppercase `0X`/`0O` prefixes, the legacy
/// leading-zero decimal) are rejected by the lexer's `read_number`, so they
/// never reach this decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// `pub(crate)`: `parse_int_literal` is re-exported at that visibility for
// the statement normalizer (ADR-0503), and its error type travels with it.
pub(crate) enum IntLiteralError {
    /// The digits do not fit the target integer range.
    Overflow,
}

/// Decode an integer literal string: decimal, or a Cypher 25 `0x`/`0o`
/// prefixed form. Pure decoding — the spelling policy is the lexer's.
/// `pub(crate)` beyond the parser module for the statement normalizer
/// (ADR-0503), which extracts the same value the parser would bind.
pub(crate) fn parse_int_literal(s: &str) -> Result<i64, IntLiteralError> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, s),
    };
    let radix = match digits.as_bytes() {
        [b'0', b'x', ..] => 16,
        [b'0', b'o', ..] => 8,
        _ => 0,
    };
    if radix == 0 {
        return s.parse::<i64>().map_err(|_| IntLiteralError::Overflow);
    }
    // `from_str_radix` takes the sign itself, so the negative/`i64::MIN`
    // cases need no separate arithmetic.
    let magnitude = &digits[2..];
    let text = if neg {
        format!("-{magnitude}")
    } else {
        magnitude.to_string()
    };
    i64::from_str_radix(&text, radix).map_err(|_| IntLiteralError::Overflow)
}

/// Where an integer literal was parsed, for the out-of-range text.
#[derive(Clone, Copy)]
pub(super) enum IntLiteralSite<'a> {
    /// An ordinary integer expression.
    Expression,
    /// A bounded position (a relationship hop count, a quantifier bound, …):
    /// the text names the bound and its maximum.
    Bounded { what: &'a str, max: u64 },
}

impl<'a> Parser<'a> {
    /// The one parse-error rendering of an [`IntLiteralError`]
    /// (`IntegerOverflow`). `site` names a bounded position, whose text
    /// names the bound and its maximum; an expression position uses the
    /// plain `integer overflow:` wording.
    pub(super) fn int_literal_error(
        &self,
        tok: &Token,
        error: IntLiteralError,
        site: IntLiteralSite<'_>,
    ) -> ParseError {
        let IntLiteralError::Overflow = error;
        match site {
            IntLiteralSite::Expression => self
                .error_at_token(tok, format!("integer overflow: {}", tok.literal))
                .with_detail("IntegerOverflow"),
            IntLiteralSite::Bounded { what, max } => self
                .error_at_token(
                    tok,
                    format!(
                        "{what} out of range: {} exceeds the maximum of {max}",
                        tok.literal
                    ),
                )
                .with_detail("IntegerOverflow"),
        }
    }
}
