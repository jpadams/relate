//! CIP-228 pattern parsing: the `date(value, pattern)` family and the
//! `duration(value, pattern)` two-argument form.
//!
//! Instants follow the Java `DateTimeFormatter` pattern alphabet Neo4j
//! uses (a letter outside that alphabet is a pattern error; a letter the
//! target kind has no use for is parsed and then ignored, CIP-228's
//! "subset of characters" rule). Durations follow Neo4j's own
//! `DurationValue.parsePattern`: only the duration alphabet is a field
//! token, every other character is literal text, and a field may not
//! repeat.
//!
//! Both grammars are compiled to a token list and matched sequentially
//! against the input, so a pattern that does not consume the whole input —
//! the input ending early, a literal that does not match, jumbled fields —
//! is one typed mismatch rather than a per-arm special case.

use std::collections::BTreeSet;

use chrono::{Datelike, Duration as ChronoDur, NaiveDate, NaiveTime, Offset, Weekday};
use chrono_tz::Tz;

use crate::{Error, Result};

use super::construct::date_from_week;
use super::zone::{
    ambiguous_datetime_err, parse_offset, resolve_local, sub_offset, unknown_zone_err,
    zone_from_short_name, LocalResolution,
};
use super::{
    range_err, Temporal, NANOS_PER_DAY, NANOS_PER_HOUR, NANOS_PER_MILLI, NANOS_PER_MINUTE,
    NANOS_PER_SECOND,
};

/// The one spelling of a pattern/value failure. Neo4j reports pattern
/// parse and match failures as `InvalidArgumentValue`; the harness's GQL
/// tables and the CIP-228 suite both accept that class.
pub(super) fn invalid_value(detail: impl std::fmt::Display) -> Error {
    Error::ty(format!("InvalidArgumentValue: {detail}"))
}

pub(super) fn mismatch() -> Error {
    invalid_value("mismatched pattern")
}

/// Neo4j rejects a pattern that names the same field twice (CIP-228: "a
/// field may not occur multiple times"): two occurrences cannot both be
/// satisfied by one input position. One spelling for both grammars.
pub(super) fn duplicate_field_err() -> Error {
    invalid_value("ambiguous pattern: duplicate field")
}

fn pattern_error(detail: impl std::fmt::Display) -> Error {
    invalid_value(format!("invalid pattern: {detail}"))
}

// ---------------------------------------------------------------------------
// Instant patterns
// ---------------------------------------------------------------------------

/// One field the Java pattern alphabet names. Aliases share a kind
/// (`y`/`u` are both [`Self::Year`]), so duplicate detection and resolution
/// read one vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FieldKind {
    Year,
    WeekYear,
    Month,
    Day,
    DayOfYear,
    Week,
    WeekOfMonth,
    Quarter,
    DayOfWeekNum,
    DayOfWeekName,
    AlignedDayOfWeek,
    ModifiedJulianDay,
    HourOfDay,
    ClockHour,
    HourOfAmPm,
    Minute,
    Second,
    Fraction,
    NanoOfSecond,
    MilliOfDay,
    NanoOfDay,
    AmPm,
    PeriodOfDay,
    Era,
    ZoneId,
    ZoneName,
    OffsetFractional,
    OffsetZ,
    OffsetLocalized,
}

impl FieldKind {
    /// The Java `DateTimeFormatter` pattern letters Neo4j accepts. A
    /// letter outside this table is an invalid pattern character.
    fn from_letter(letter: char) -> Option<Self> {
        Some(match letter {
            'G' => Self::Era,
            'y' | 'u' => Self::Year,
            'Y' => Self::WeekYear,
            'M' | 'L' => Self::Month,
            'd' => Self::Day,
            'D' => Self::DayOfYear,
            'w' => Self::Week,
            'W' => Self::WeekOfMonth,
            'Q' | 'q' => Self::Quarter,
            'e' | 'c' => Self::DayOfWeekNum,
            'E' => Self::DayOfWeekName,
            'F' => Self::AlignedDayOfWeek,
            'g' => Self::ModifiedJulianDay,
            'H' => Self::HourOfDay,
            'k' => Self::ClockHour,
            'h' | 'K' => Self::HourOfAmPm,
            'm' => Self::Minute,
            's' => Self::Second,
            'S' => Self::Fraction,
            'A' => Self::MilliOfDay,
            'n' => Self::NanoOfSecond,
            'N' => Self::NanoOfDay,
            'a' => Self::AmPm,
            'B' => Self::PeriodOfDay,
            'V' | 'v' => Self::ZoneId,
            'z' => Self::ZoneName,
            'X' | 'x' => Self::OffsetFractional,
            'Z' => Self::OffsetZ,
            'O' => Self::OffsetLocalized,
            _ => return None,
        })
    }
}

/// One compiled instant-pattern token.
#[derive(Debug, Clone)]
enum Token {
    Field {
        kind: FieldKind,
        /// The spelling that produced the field (`y` vs `u`), which the
        /// era rule needs: only year-of-era (`y`) shifts for `BC`.
        letter: char,
        count: usize,
        /// The `p` pad modifier's width: the token consumes exactly this
        /// many characters, leading spaces included.
        pad: usize,
    },
    Literal(String),
    /// A `[...]` section: matched when it matches, otherwise skipped.
    Optional(Vec<Token>),
}

/// Maximum `[` section nesting accepted in a pattern, matching the query
/// parser's `MAX_NESTING_DEPTH` of 500 (that crate is not linkable from
/// here, so the number is restated). The compiled token tree is walked
/// recursively by [`reject_duplicate_fields`] and [`match_tokens`];
/// [`tokenize_section`] is the only constructor of [`Token::Optional`], so
/// refusing the pattern here bounds all three walks by construction. A
/// deeper pattern is a typed error instead of a stack overflow on the
/// connection thread.
const MAX_PATTERN_NESTING: usize = 500;

fn tokenize(pattern: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    let tokens = tokenize_section(&chars, &mut index, 0)?;
    reject_duplicate_fields(&tokens)?;
    Ok(tokens)
}

/// `depth` is the number of enclosing `[` sections; zero at the top level.
fn tokenize_section(chars: &[char], index: &mut usize, depth: usize) -> Result<Vec<Token>> {
    let in_optional = depth > 0;
    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut pad = 0usize;
    let flush = |literal: &mut String, tokens: &mut Vec<Token>| {
        if !literal.is_empty() {
            tokens.push(Token::Literal(std::mem::take(literal)));
        }
    };
    while *index < chars.len() {
        let c = chars[*index];
        match c {
            '\'' => {
                *index += 1;
                loop {
                    let Some(&q) = chars.get(*index) else {
                        return Err(pattern_error("unterminated quote"));
                    };
                    if q == '\'' {
                        if chars.get(*index + 1) == Some(&'\'') {
                            literal.push('\'');
                            *index += 2;
                            continue;
                        }
                        *index += 1;
                        break;
                    }
                    literal.push(q);
                    *index += 1;
                }
            }
            ']' if in_optional => {
                flush(&mut literal, &mut tokens);
                *index += 1;
                return Ok(tokens);
            }
            '[' => {
                if pad != 0 {
                    return Err(pattern_error("pad modifier has no field"));
                }
                if depth >= MAX_PATTERN_NESTING {
                    return Err(pattern_error("pattern nesting too deep"));
                }
                flush(&mut literal, &mut tokens);
                *index += 1;
                let inner = tokenize_section(chars, index, depth + 1)?;
                tokens.push(Token::Optional(inner));
            }
            ']' => return Err(pattern_error("unmatched ']'")),
            c if c.is_ascii_alphabetic() => {
                let start = *index;
                while chars.get(*index) == Some(&c) {
                    *index += 1;
                }
                let count = *index - start;
                if c == 'p' {
                    pad = count;
                    continue;
                }
                let kind = FieldKind::from_letter(c)
                    .ok_or_else(|| pattern_error(format!("unknown pattern letter '{c}'")))?;
                flush(&mut literal, &mut tokens);
                tokens.push(Token::Field {
                    kind,
                    letter: c,
                    count,
                    pad: std::mem::take(&mut pad),
                });
            }
            _ => {
                literal.push(c);
                *index += 1;
            }
        }
    }
    if in_optional {
        return Err(pattern_error("unterminated '['"));
    }
    if pad != 0 {
        return Err(pattern_error("pad modifier has no field"));
    }
    flush(&mut literal, &mut tokens);
    Ok(tokens)
}

fn reject_duplicate_fields(tokens: &[Token]) -> Result<()> {
    let mut seen = BTreeSet::new();
    fn walk(tokens: &[Token], seen: &mut BTreeSet<FieldKind>) -> Result<()> {
        for token in tokens {
            match token {
                Token::Field { kind, .. } => {
                    if !seen.insert(*kind) {
                        return Err(duplicate_field_err());
                    }
                }
                Token::Optional(inner) => walk(inner, seen)?,
                Token::Literal(_) => {}
            }
        }
        Ok(())
    }
    walk(tokens, &mut seen)
}

/// The field values an instant pattern collected, before the target kind
/// decides which of them it uses (CIP-228: a character outside the kind's
/// checkmarks is parsed but ignored).
#[derive(Default, Clone)]
struct Fields {
    year: Option<i64>,
    week_year: Option<i64>,
    month: Option<u32>,
    day: Option<u32>,
    day_of_year: Option<u32>,
    week: Option<u32>,
    quarter: Option<u32>,
    day_of_week: Option<u32>,
    day_of_week_name: Option<Weekday>,
    aligned_day_of_week: Option<u32>,
    modified_julian: Option<i64>,
    hour_of_day: Option<u32>,
    clock_hour: Option<u32>,
    hour_of_am_pm: Option<u32>,
    minute: Option<u32>,
    second: Option<u32>,
    nano: Option<u32>,
    milli_of_day: Option<i64>,
    nano_of_day: Option<i64>,
    am_pm: Option<bool>,
    /// The `y` spelling is year-of-era; `u` is the proleptic year.
    year_of_era: bool,
    /// A parsed `BC`/`BCE` era shifts a year-of-era by one.
    era_bc: bool,
    offset: Option<i32>,
    zone: Option<String>,
}

impl Fields {
    /// Whether the pattern named a time-of-day field — derived from the
    /// collected fields rather than tracked beside them.
    fn has_time(&self) -> bool {
        self.hour_of_day.is_some()
            || self.clock_hour.is_some()
            || self.hour_of_am_pm.is_some()
            || self.minute.is_some()
            || self.second.is_some()
            || self.nano.is_some()
            || self.milli_of_day.is_some()
            || self.nano_of_day.is_some()
    }
}

/// `Ok(None)` is a shape mismatch (the input does not fit the pattern);
/// `Err` is a value the grammar parsed but cannot represent (a component
/// that overflows `i64`), which must not read as a mismatch.
fn match_tokens(
    tokens: &[Token],
    input: &str,
    pos: usize,
    fields: &mut Fields,
) -> Result<Option<usize>> {
    let mut pos = pos;
    for token in tokens {
        match token {
            Token::Literal(text) => {
                if read_literal(input, &mut pos, text).is_none() {
                    return Ok(None);
                }
            }
            Token::Optional(inner) => {
                let mut trial = fields.clone();
                if let Some(end) = match_tokens(inner, input, pos, &mut trial)? {
                    *fields = trial;
                    pos = end;
                }
            }
            Token::Field {
                kind,
                letter,
                count,
                pad,
            } => {
                let Some(end) = read_field(*kind, *letter, *count, *pad, input, pos, fields)?
                else {
                    return Ok(None);
                };
                pos = end;
            }
        }
    }
    Ok(Some(pos))
}

fn read_field(
    kind: FieldKind,
    letter: char,
    count: usize,
    pad: usize,
    input: &str,
    mut pos: usize,
    fields: &mut Fields,
) -> Result<Option<usize>> {
    let number = |pos: &mut usize, sign: bool| read_number(input, pos, field_width(count), sign);
    match kind {
        FieldKind::Year => {
            let raw = if pad == 0 {
                read_number(input, &mut pos, field_width(count), false)?
            } else {
                read_padded_number(input, &mut pos, pad)?
            };
            let Some(raw) = raw else { return Ok(None) };
            fields.year_of_era = letter == 'y';
            fields.year = Some(if count == 2 {
                2000 + raw.value.rem_euclid(100)
            } else {
                raw.value
            });
        }
        FieldKind::WeekYear => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.week_year = Some(raw.value);
        }
        FieldKind::Month => {
            if count >= 3 {
                let Some(name) = read_alpha(input, &mut pos) else {
                    return Ok(None);
                };
                let Some(month) = month_from_name(name) else {
                    return Ok(None);
                };
                fields.month = Some(month);
            } else {
                let Some(raw) = number(&mut pos, false)? else {
                    return Ok(None);
                };
                fields.month = Some(as_u32(raw.value)?);
            }
        }
        FieldKind::Day => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.day = Some(as_u32(raw.value)?);
        }
        FieldKind::DayOfYear => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.day_of_year = Some(as_u32(raw.value)?);
        }
        FieldKind::Week => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.week = Some(as_u32(raw.value)?);
        }
        FieldKind::WeekOfMonth => {
            // Defined but does not contribute to the kinds this engine
            // constructs; still consumes its input.
            if number(&mut pos, false)?.is_none() {
                return Ok(None);
            }
        }
        FieldKind::Quarter => {
            if count >= 4 {
                let Some(quarter) = read_quarter(input, &mut pos) else {
                    return Ok(None);
                };
                fields.quarter = Some(quarter);
            } else {
                let Some(raw) = number(&mut pos, false)? else {
                    return Ok(None);
                };
                fields.quarter = Some(as_u32(raw.value)?);
            }
        }
        FieldKind::DayOfWeekNum => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.day_of_week = Some(as_u32(raw.value)?);
        }
        FieldKind::DayOfWeekName => {
            let Some(name) = read_alpha(input, &mut pos) else {
                return Ok(None);
            };
            let Some(weekday) = weekday_from_name(name) else {
                return Ok(None);
            };
            fields.day_of_week_name = Some(weekday);
        }
        FieldKind::AlignedDayOfWeek => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.aligned_day_of_week = Some(as_u32(raw.value)?);
        }
        FieldKind::ModifiedJulianDay => {
            let Some(raw) = number(&mut pos, true)? else {
                return Ok(None);
            };
            fields.modified_julian = Some(raw.value);
        }
        FieldKind::HourOfDay => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.hour_of_day = Some(as_u32(raw.value)?);
        }
        FieldKind::ClockHour => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.clock_hour = Some(as_u32(raw.value)?);
        }
        FieldKind::HourOfAmPm => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.hour_of_am_pm = Some(as_u32(raw.value)?);
        }
        FieldKind::Minute => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.minute = Some(as_u32(raw.value)?);
        }
        FieldKind::Second => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.second = Some(as_u32(raw.value)?);
        }
        FieldKind::Fraction => {
            let width = count.min(9);
            let Some(raw) = read_number(input, &mut pos, Some(width), false)? else {
                return Ok(None);
            };
            fields.nano = Some(as_u32(nanos_from_fraction(raw.value, raw.digits)?)?);
        }
        FieldKind::NanoOfSecond => {
            let Some(raw) = number(&mut pos, false)? else {
                return Ok(None);
            };
            fields.nano = Some(as_u32(raw.value)?);
        }
        FieldKind::MilliOfDay => {
            let Some(raw) = number(&mut pos, true)? else {
                return Ok(None);
            };
            fields.milli_of_day = Some(raw.value);
        }
        FieldKind::NanoOfDay => {
            let Some(raw) = number(&mut pos, true)? else {
                return Ok(None);
            };
            fields.nano_of_day = Some(raw.value);
        }
        FieldKind::AmPm => {
            let Some(marker) = read_alpha(input, &mut pos) else {
                return Ok(None);
            };
            fields.am_pm = Some(match marker.to_ascii_uppercase().as_str() {
                "AM" => false,
                "PM" => true,
                _ => return Ok(None),
            });
        }
        FieldKind::PeriodOfDay => {
            // A day-period phrase is informational; it consumes the rest of
            // the input and contributes nothing to the value.
            pos = input.len();
        }
        FieldKind::Era => {
            let Some(era) = read_alpha(input, &mut pos) else {
                return Ok(None);
            };
            fields.era_bc = match era.to_ascii_uppercase().as_str() {
                "AD" | "CE" => false,
                "BC" | "BCE" => true,
                _ => return Ok(None),
            };
        }
        FieldKind::ZoneId => {
            let Some(name) = read_non_space(input, &mut pos) else {
                return Ok(None);
            };
            // `VV` is a zone id: an IANA region, or a `ZoneOffset` id such
            // as `+02:00` (java.time's `ZoneId` admits both). Anything else
            // does not match this field.
            if name.starts_with(['+', '-']) {
                let Some(offset) = parse_offset(name).ok() else {
                    return Ok(None);
                };
                fields.offset = Some(offset);
            } else {
                if name.parse::<Tz>().is_err() {
                    return Ok(None);
                }
                fields.zone = Some(name.to_string());
            }
        }
        FieldKind::ZoneName => {
            let Some(name) = read_non_space(input, &mut pos) else {
                return Ok(None);
            };
            let zone = match zone_from_short_name(name) {
                Some(zone) => zone.to_string(),
                None => {
                    // A full IANA id is also a legitimate `z` value.
                    if name.parse::<Tz>().is_err() {
                        return Ok(None);
                    }
                    name.to_string()
                }
            };
            fields.zone = Some(zone);
        }
        FieldKind::OffsetFractional | FieldKind::OffsetZ => {
            let Some(offset) = read_offset(input, &mut pos) else {
                return Ok(None);
            };
            fields.offset = Some(offset);
        }
        FieldKind::OffsetLocalized => {
            // `O` is the localized offset spelling: a `GMT`/`UTC` prefix
            // and an optional numeric offset (`GMT+2`, `UTC-08:00`).
            let Some(raw) = read_non_space(input, &mut pos) else {
                return Ok(None);
            };
            let rest = raw.strip_prefix("GMT").or_else(|| raw.strip_prefix("UTC"));
            let Some(rest) = rest else { return Ok(None) };
            fields.offset = Some(if rest.is_empty() {
                0
            } else {
                let Some(offset) = parse_offset(rest).ok() else {
                    return Ok(None);
                };
                offset
            });
        }
    }
    Ok(Some(pos))
}

fn as_u32(value: i64) -> Result<u32> {
    u32::try_from(value).map_err(|_| range_err())
}

/// The input width a field token consumes: a repeated letter run (`MM`,
/// `yyyy`) is fixed-width; a single letter consumes the whole digit run.
fn field_width(count: usize) -> Option<usize> {
    (count > 1).then_some(count)
}

/// One numeric field read: the parsed value and the width of its digit run
/// (the fraction field pads by that width).
pub(super) struct Digits {
    pub(super) value: i64,
    pub(super) digits: usize,
}

/// Read up to `exact` digits (a run when `None`). With `sign`, a leading
/// `+`/`-` belongs to the number. `Ok(None)` is a shape mismatch; the
/// digits are ASCII and shape-checked, so a parse failure is overflow.
///
/// The position advances only on `Ok(Some)`: a mismatch (including a
/// consumed sign) or an overflow restores it, the same contract as
/// [`read_literal`]. Every caller propagates the failure immediately, so
/// nothing depended on the old advance-on-failure behaviour.
pub(super) fn read_number(
    input: &str,
    pos: &mut usize,
    exact: Option<usize>,
    sign: bool,
) -> Result<Option<Digits>> {
    let start = *pos;
    if sign && matches!(input.as_bytes().get(*pos), Some(b'+' | b'-')) {
        *pos += 1;
    }
    let digits_start = *pos;
    let limit = exact.unwrap_or(usize::MAX);
    while *pos < input.len()
        && *pos - digits_start < limit
        && input.as_bytes()[*pos].is_ascii_digit()
    {
        *pos += 1;
    }
    let digits = *pos - digits_start;
    let satisfied = match exact {
        Some(want) => digits == want,
        None => digits > 0,
    };
    if !satisfied {
        *pos = start;
        return Ok(None);
    }
    let Some(text) = input.get(start..*pos) else {
        *pos = start;
        return Ok(None);
    };
    let value = text.parse().map_err(|_| {
        *pos = start;
        range_err()
    })?;
    Ok(Some(Digits { value, digits }))
}

/// The nanosecond value of a fraction's digits: `digits` fraction digits
/// name that many tenths-powers of a second, so the value is right-padded
/// with zeros to nine places (`87` → 870_000_000 ns). The one spelling of
/// the pad rule for both pattern grammars.
pub(super) fn nanos_from_fraction(value: i64, digits: usize) -> Result<i64> {
    if digits >= 9 {
        return Ok(value);
    }
    let scale = 10i64
        .checked_pow((9 - digits) as u32)
        .ok_or_else(range_err)?;
    value.checked_mul(scale).ok_or_else(range_err)
}

/// A `p`-padded field: exactly `width` characters, leading spaces ignored.
fn read_padded_number(input: &str, pos: &mut usize, width: usize) -> Result<Option<Digits>> {
    let Some(text) = input.get(*pos..*pos + width) else {
        return Ok(None);
    };
    let trimmed = text.trim_start();
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let value = trimmed.parse().map_err(|_| range_err())?;
    *pos += width;
    Ok(Some(Digits {
        value,
        digits: trimmed.len(),
    }))
}

/// Take the longest run of characters `take` accepts; `None` when the run
/// is empty.
fn read_while<'a>(input: &'a str, pos: &mut usize, take: impl Fn(char) -> bool) -> Option<&'a str> {
    let start = *pos;
    while let Some(c) = input[*pos..].chars().next() {
        if !take(c) {
            break;
        }
        *pos += c.len_utf8();
    }
    (*pos > start).then(|| &input[start..*pos])
}

/// Consume exactly `text` at `pos`; `None` (position unchanged) when the
/// input does not carry it. The one literal-consumption rule for both
/// pattern grammars.
pub(super) fn read_literal(input: &str, pos: &mut usize, text: &str) -> Option<()> {
    if !input.get(*pos..).is_some_and(|rest| rest.starts_with(text)) {
        return None;
    }
    *pos += text.len();
    Some(())
}

fn read_alpha<'a>(input: &'a str, pos: &mut usize) -> Option<&'a str> {
    read_while(input, pos, char::is_alphabetic)
}

fn read_non_space<'a>(input: &'a str, pos: &mut usize) -> Option<&'a str> {
    read_while(input, pos, |c| !c.is_whitespace())
}

/// `QQQQ`: a quarter ordinal plus the literal word `quarter`, e.g.
/// `2nd quarter`. The ordinal suffix is not inspected; the number is.
fn read_quarter(input: &str, pos: &mut usize) -> Option<u32> {
    let quarter: u32 = read_while(input, pos, |c| c.is_ascii_digit())?
        .parse()
        .ok()?;
    // The ordinal suffix is not inspected: it is consumed and dropped.
    let _ = read_alpha(input, pos);
    read_while(input, pos, |c| c == ' ')?;
    let word = input.get(*pos..)?.get(..7)?;
    if !word.eq_ignore_ascii_case("quarter") {
        return None;
    }
    *pos += 7;
    Some(quarter)
}

/// A `X`/`x`/`Z` zone offset: `Z` for UTC, otherwise a signed `HH`,
/// `HHMM`, `HH:MM` (optionally with seconds) run.
fn read_offset(input: &str, pos: &mut usize) -> Option<i32> {
    if matches!(input.as_bytes().get(*pos), Some(b'Z' | b'z')) {
        *pos += 1;
        return Some(0);
    }
    let start = *pos;
    if matches!(input.as_bytes().get(*pos), Some(b'+' | b'-')) {
        *pos += 1;
    }
    read_while(input, pos, |c| c.is_ascii_digit() || c == ':')?;
    parse_offset(input.get(start..*pos)?).ok()
}

/// Look a name up in a `(value, full name)` table, case-insensitively and
/// accepting the three-letter abbreviation (Java's `MMM`/`EEE` short
/// forms) — the one spelling of the name rule both tables read.
fn match_name<T: Copy>(name: &str, table: &[(T, &str)]) -> Option<T> {
    let lowered = name.to_ascii_lowercase();
    table
        .iter()
        .find(|(_, full)| *full == lowered || (name.len() == 3 && full.starts_with(&lowered)))
        .map(|(value, _)| *value)
}

fn month_from_name(name: &str) -> Option<u32> {
    const MONTHS: [(u32, &str); 12] = [
        (1, "january"),
        (2, "february"),
        (3, "march"),
        (4, "april"),
        (5, "may"),
        (6, "june"),
        (7, "july"),
        (8, "august"),
        (9, "september"),
        (10, "october"),
        (11, "november"),
        (12, "december"),
    ];
    match_name(name, &MONTHS)
}

fn weekday_from_name(name: &str) -> Option<Weekday> {
    const DAYS: [(Weekday, &str); 7] = [
        (Weekday::Mon, "monday"),
        (Weekday::Tue, "tuesday"),
        (Weekday::Wed, "wednesday"),
        (Weekday::Thu, "thursday"),
        (Weekday::Fri, "friday"),
        (Weekday::Sat, "saturday"),
        (Weekday::Sun, "sunday"),
    ];
    match_name(name, &DAYS)
}

/// The localized day-of-week (`e`/`c`): Sunday is 1, Saturday 7 — the
/// number-to-weekday mapping `format.rs` renders with
/// `num_days_from_sunday() + 1`.
fn weekday_from_localized(value: u32) -> Result<Weekday> {
    Ok(match value {
        1 => Weekday::Sun,
        2 => Weekday::Mon,
        3 => Weekday::Tue,
        4 => Weekday::Wed,
        5 => Weekday::Thu,
        6 => Weekday::Fri,
        7 => Weekday::Sat,
        _ => return Err(invalid_value("day-of-week out of range")),
    })
}

/// `date(value, pattern)` and the other instant kinds.
pub(super) fn parse_instant_pattern(kind: &str, value: &str, pattern: &str) -> Result<Temporal> {
    let tokens = tokenize(pattern)?;
    let mut fields = Fields::default();
    let end = match_tokens(&tokens, value, 0, &mut fields)?.ok_or_else(mismatch)?;
    if end != value.len() {
        return Err(mismatch());
    }
    match kind {
        "date" => Ok(Temporal::Date(resolve_date(&fields)?)),
        "localtime" => Ok(Temporal::LocalTime(resolve_time(&fields)?)),
        "time" => Ok(Temporal::Time {
            time: resolve_time(&fields)?,
            offset: fields.offset.unwrap_or(0),
        }),
        "localdatetime" => Ok(Temporal::LocalDateTime(
            resolve_date(&fields)?.and_time(resolve_time(&fields)?),
        )),
        "datetime" => {
            let date = resolve_date(&fields)?;
            let time = resolve_time(&fields)?;
            let local = date.and_time(time);
            if let Some(zone) = fields.zone.as_deref() {
                let tz: Tz = zone.parse().map_err(|_| unknown_zone_err(zone))?;
                let resolved = match resolve_local(tz, local) {
                    LocalResolution::Unique(dt) => dt,
                    // ADR-0178's overlap rule: the earlier occurrence.
                    LocalResolution::Ambiguous { earlier, .. } => earlier,
                    LocalResolution::None => return Err(ambiguous_datetime_err()),
                };
                return Ok(Temporal::DateTime {
                    utc: resolved.naive_utc(),
                    offset: resolved.offset().fix().local_minus_utc(),
                    zone: Some(zone.to_string()),
                });
            }
            if fields.has_time() {
                let offset = fields.offset.unwrap_or(0);
                return Ok(Temporal::DateTime {
                    utc: sub_offset(local, offset)?,
                    offset,
                    zone: fields.offset.is_none().then(|| "Z".into()),
                });
            }
            // A date-only pattern: a parsed offset does not apply (CIP-228
            // instant semantics), so the value is midnight UTC.
            Ok(Temporal::DateTime {
                utc: local,
                offset: 0,
                zone: Some("Z".into()),
            })
        }
        other => Err(invalid_value(format!("unknown temporal kind {other}"))),
    }
}

fn resolve_date(fields: &Fields) -> Result<NaiveDate> {
    if let Some(mjd) = fields.modified_julian {
        let epoch = NaiveDate::from_ymd_opt(1858, 11, 17).ok_or_else(range_err)?;
        // `ChronoDur::days` panics past chrono's duration bound; an mjd that
        // parses as `i64` is query input, so it must be a typed range error.
        let days = ChronoDur::try_days(mjd).ok_or_else(range_err)?;
        return epoch.checked_add_signed(days).ok_or_else(range_err);
    }
    if let (Some(year), Some(week), Some(day)) = (fields.week_year, fields.week, fields.day_of_week)
    {
        let year = i32::try_from(year).map_err(|_| range_err())?;
        return date_from_week(year, week, weekday_from_localized(day)?);
    }
    // A `BC`/`BCE` era shifts a year-of-era (`y`) by one: 44 BC is
    // proleptic year -43. A proleptic `u` year is already absolute.
    let year = fields.year.map(|year| {
        if fields.year_of_era && fields.era_bc {
            1 - year
        } else {
            year
        }
    });
    if let (Some(year), Some(ordinal)) = (year, fields.day_of_year) {
        let date = NaiveDate::from_yo_opt(i32::try_from(year).map_err(|_| range_err())?, ordinal)
            .ok_or_else(|| invalid_value("invalid date"))?;
        check_day_of_week(fields, date)?;
        return Ok(date);
    }
    if let (Some(year), Some(month), Some(aligned), Some(day)) = (
        year,
        fields.month,
        fields.aligned_day_of_week,
        fields.day_of_week,
    ) {
        let year = i32::try_from(year).map_err(|_| range_err())?;
        let first =
            NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(|| invalid_value("invalid date"))?;
        let weekday = weekday_from_localized(day)?;
        let offset = i64::from(weekday.num_days_from_monday())
            - i64::from(first.weekday().num_days_from_monday());
        let day_of_month = 1 + offset.rem_euclid(7) + (i64::from(aligned) - 1) * 7;
        let day_of_month = u32::try_from(day_of_month).map_err(|_| range_err())?;
        return NaiveDate::from_ymd_opt(year, month, day_of_month)
            .ok_or_else(|| invalid_value("invalid date"));
    }
    if let (Some(year), Some(month), Some(day)) = (year, fields.month, fields.day) {
        let date =
            NaiveDate::from_ymd_opt(i32::try_from(year).map_err(|_| range_err())?, month, day)
                .ok_or_else(|| invalid_value("invalid date"))?;
        check_day_of_week(fields, date)?;
        return Ok(date);
    }
    if let (Some(year), Some(quarter)) = (year, fields.quarter) {
        if !(1..=4).contains(&quarter) {
            return Err(invalid_value("quarter out of range"));
        }
        return NaiveDate::from_ymd_opt(
            i32::try_from(year).map_err(|_| range_err())?,
            (quarter - 1) * 3 + 1,
            1,
        )
        .ok_or_else(|| invalid_value("invalid date"));
    }
    Err(invalid_value("incomplete date"))
}

fn check_day_of_week(fields: &Fields, date: NaiveDate) -> Result<()> {
    if let Some(day) = fields.day_of_week {
        if weekday_from_localized(day)? != date.weekday() {
            return Err(invalid_value("conflicting day of week"));
        }
    }
    if let Some(weekday) = fields.day_of_week_name {
        if weekday != date.weekday() {
            return Err(invalid_value("conflicting day of week"));
        }
    }
    Ok(())
}

fn resolve_time(fields: &Fields) -> Result<NaiveTime> {
    let mut hour: i64 = 0;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    let mut nano: i64 = 0;
    // The two day-relative spellings name one nanosecond-of-day total, so
    // it is decomposed once; each spelling keeps its own name in the range
    // error. The unit constants are `i128` (the duration arithmetic's
    // width) and fit `i64` here: a day is 8.6e13 ns against 9.2e18.
    let of_day = if let Some(total) = fields.nano_of_day {
        if !(0..NANOS_PER_DAY).contains(&i128::from(total)) {
            return Err(invalid_value("nano-of-day out of range"));
        }
        Some(i128::from(total))
    } else if let Some(millis) = fields.milli_of_day {
        if !(0..NANOS_PER_DAY / NANOS_PER_MILLI).contains(&i128::from(millis)) {
            return Err(invalid_value("milli-of-day out of range"));
        }
        Some(i128::from(millis) * NANOS_PER_MILLI)
    } else {
        None
    };
    if let Some(total) = of_day {
        let total = i64::try_from(total).map_err(|_| range_err())?;
        let (hour_nanos, minute_nanos, second_nanos) = (
            NANOS_PER_HOUR as i64,
            NANOS_PER_MINUTE as i64,
            NANOS_PER_SECOND as i64,
        );
        hour = total / hour_nanos;
        minute = total % hour_nanos / minute_nanos;
        second = total % minute_nanos / second_nanos;
        nano = total % second_nanos;
    }
    if let Some(value) = fields.hour_of_day {
        if value > 23 {
            return Err(invalid_value("hour out of range"));
        }
        hour = i64::from(value);
    }
    if let Some(value) = fields.clock_hour {
        let value = if value == 24 { 0 } else { value };
        if value > 23 {
            return Err(invalid_value("hour out of range"));
        }
        hour = i64::from(value);
    }
    if let Some(value) = fields.hour_of_am_pm {
        if value > 12 {
            return Err(invalid_value("hour out of range"));
        }
        // A 12-hour clock needs its AM/PM marker: Java's resolver cannot
        // build a time from hour-of-am-pm alone, and the mismatch is the
        // pattern's, not a silent AM.
        let pm = fields
            .am_pm
            .ok_or_else(|| invalid_value("hour-of-am-pm without AM/PM marker"))?;
        hour = i64::from(value % 12) + if pm { 12 } else { 0 };
    }
    if let Some(value) = fields.minute {
        if value > 59 {
            return Err(invalid_value("minute out of range"));
        }
        minute = i64::from(value);
    }
    if let Some(value) = fields.second {
        if value > 59 {
            return Err(invalid_value("second out of range"));
        }
        second = i64::from(value);
    }
    if let Some(value) = fields.nano {
        if value > 999_999_999 {
            return Err(invalid_value("nanosecond out of range"));
        }
        nano = i64::from(value);
    }
    NaiveTime::from_hms_nano_opt(
        as_u32(hour)?,
        as_u32(minute)?,
        as_u32(second)?,
        as_u32(nano)?,
    )
    .ok_or_else(|| invalid_value("invalid time"))
}
