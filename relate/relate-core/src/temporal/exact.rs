//! The exact duration-component arithmetic shared by the ISO numeral parser
//! ([`super::duration`]) and the map constructor ([`super::construct`]): a
//! checked `i128` integer sum plus an `f64` fractional remainder that
//! carries down the month → day → nanosecond tiers.
//!
//! The integer sum keeps components above 2^53 exact; the remainder collects
//! only fractions, so the lossy part of the arithmetic stays below the
//! duration's nanosecond resolution.

use crate::{Error, Result, Value};

use super::{f64_to_i128_range, range_err, AVG_DAYS_PER_MONTH, NANOS_PER_DAY};

/// One duration field's number, keeping an integer exact: `as_number`
/// promotes an `Int` to `f64` (losing every digit above 2^53), which
/// `duration({seconds: 9223372036854775807})` must not do.
#[derive(Clone, Copy)]
pub(super) enum ExactNumber {
    Int(i128),
    Float(f64),
}

impl ExactNumber {
    /// `true` for an exact integer field — the map constructor's
    /// all-integral split consults this instead of carrying a mode flag.
    pub(super) fn is_int(&self) -> bool {
        matches!(self, Self::Int(_))
    }

    /// The exact integer contribution, or `None` for a float field.
    pub(super) fn as_int(&self) -> Option<i128> {
        match self {
            Self::Int(value) => Some(*value),
            Self::Float(_) => None,
        }
    }
}

/// One duration-map entry's number as an [`ExactNumber`]: a field present
/// but not numeric is the typed argument error the constructor uses —
/// never a silent zero contribution (`duration({seconds: '60'})` must not
/// answer `PT0S`).
pub(super) fn exact_number(key: &str, value: &Value) -> Result<ExactNumber> {
    match value {
        Value::Int(v) => Ok(ExactNumber::Int(i128::from(*v))),
        Value::Float(v) => Ok(ExactNumber::Float(*v)),
        _ => Err(Error::argument_error(format!(
            "duration field `{key}` must be a number"
        ))),
    }
}

/// A duration-component accumulator: an exact integer sum plus an `f64`
/// fractional remainder. [`Self::carry_fraction`] moves whole units down to
/// the next (finer) tier, mirroring the constructor's trunc/fract cascade.
#[derive(Default)]
pub(super) struct ExactAccumulator {
    whole: i128,
    frac: f64,
}

impl ExactAccumulator {
    /// Add one field's number under its unit `scale`.
    pub(super) fn add(&mut self, number: ExactNumber, scale: i128) -> Result<()> {
        match number {
            ExactNumber::Int(v) => {
                add_exact_nanos(&mut self.whole, v.checked_mul(scale).ok_or_else(range_err)?)
            }
            ExactNumber::Float(v) => self.add_float(v * scale as f64),
        }
    }

    /// Add one parsed numeral's whole part: `whole * scale` exactly. The
    /// grammar applies the numeral's sign before the call, so every add
    /// path here takes a signed value (the same convention as [`Self::add`]).
    pub(super) fn add_whole(&mut self, whole: i128, scale: i128) -> Result<()> {
        add_exact_nanos(
            &mut self.whole,
            whole.checked_mul(scale).ok_or_else(range_err)?,
        )
    }

    /// Add one parsed numeral component: [`Self::add_whole`] for the integer
    /// part, plus the fractional part `frac * scale` through
    /// [`Self::add_float`] — the one unit helper (hour/minute/second and the
    /// week/day tiers alike), with the `f64` scale derived from the integer.
    pub(super) fn add_unit(&mut self, whole: i128, frac: f64, scale: i128) -> Result<()> {
        self.add_whole(whole, scale)?;
        self.add_float(frac * scale as f64)
    }

    /// Add an already-scaled value, splitting it into whole units and a
    /// fractional remainder.
    pub(super) fn add_float(&mut self, value: f64) -> Result<()> {
        let whole_part = value.trunc();
        add_exact_nanos(&mut self.whole, f64_to_i128_range(whole_part)?)?;
        self.frac += value - whole_part;
        Ok(())
    }

    /// The accumulated whole units, range-checked to the `i64` the duration
    /// model stores.
    pub(super) fn whole_i64(&self) -> Result<i64> {
        i64::try_from(self.whole).map_err(|_| range_err())
    }

    /// Move any whole units the accumulated fractions added up to into
    /// [`Self::whole`], returning what is left (in `(-1, 1)`).
    pub(super) fn carry_fraction(&mut self) -> Result<f64> {
        let carried = self.frac.trunc();
        add_exact_nanos(&mut self.whole, f64_to_i128_range(carried)?)?;
        self.frac -= carried;
        Ok(self.frac)
    }

    /// The total rounded to whole units — the nanosecond tier's final value.
    pub(super) fn rounded(&self) -> Result<i128> {
        self.whole
            .checked_add(f64_to_i128_range(self.frac.round())?)
            .ok_or_else(range_err)
    }

    /// The total truncated to whole units — the nanosecond tier's final
    /// value where the caller must not round up (duration scaling).
    pub(super) fn truncated(&self) -> Result<i128> {
        self.whole
            .checked_add(f64_to_i128_range(self.frac.trunc())?)
            .ok_or_else(range_err)
    }
}

/// Checked addition into an `i128` component accumulator.
pub(super) fn add_exact_nanos(dst: &mut i128, delta: i128) -> Result<()> {
    *dst = dst.checked_add(delta).ok_or_else(range_err)?;
    Ok(())
}

/// The three tiers a duration accumulates into, plus the one carry/fract
/// cascade: a fractional month becomes days, a fractional day becomes
/// nanoseconds, then each tier folds to its stored width (`i64` months and
/// days, `i128` nanos). The ISO numeral parser
/// ([`super::duration`]), the map constructor ([`super::construct`]), and
/// duration scaling ([`super::arith`]) share it, so the cascade exists once;
/// [`Self::finish`] rounds the final nanosecond fold, [`Self::finish_trunc`]
/// truncates it for scaling.
#[derive(Default)]
pub(super) struct DurationTiers {
    pub(super) months: ExactAccumulator,
    pub(super) days: ExactAccumulator,
    pub(super) nanos: ExactAccumulator,
}

impl DurationTiers {
    /// The trunc/fract cascade down the tiers, then the stored triple, with
    /// the nanosecond tier rounded.
    pub(super) fn finish(self) -> Result<(i64, i64, i128)> {
        let (months, days, nanos) = self.cascade()?;
        Ok((months, days, nanos.rounded()?))
    }

    /// The same carry cascade with the nanosecond tier truncated instead of
    /// rounded. Duration scaling is the one caller: its factors are lossy
    /// `f64` products and the hand-rolled cascade it replaces truncated the
    /// final nanosecond sum (the pinned CIP-223/CIP-228 answers depend on
    /// that fold), so the difference is deliberate — the assembly is still
    /// this one cascade.
    pub(super) fn finish_trunc(self) -> Result<(i64, i64, i128)> {
        let (months, days, nanos) = self.cascade()?;
        Ok((months, days, nanos.truncated()?))
    }

    /// The shared trunc/fract cascade: month fraction → days, day fraction
    /// → nanoseconds, and the month/day tiers folded to their stored `i64`
    /// width. The nanosecond accumulator is returned for the caller's final
    /// fold ([`ExactAccumulator::rounded`] or [`ExactAccumulator::truncated`]).
    fn cascade(mut self) -> Result<(i64, i64, ExactAccumulator)> {
        self.days
            .add_float(self.months.carry_fraction()? * AVG_DAYS_PER_MONTH)?;
        self.nanos
            .add_float(self.days.carry_fraction()? * NANOS_PER_DAY as f64)?;
        Ok((self.months.whole_i64()?, self.days.whole_i64()?, self.nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_components_split_and_carry_down_tiers() {
        let mut days = ExactAccumulator::default();
        days.add(ExactNumber::Float(1.5), 1).unwrap();
        assert_eq!(days.whole_i64().unwrap(), 1);
        let mut nanos = ExactAccumulator::default();
        nanos
            .add_float(days.carry_fraction().unwrap() * super::super::NANOS_PER_DAY as f64)
            .unwrap();
        assert_eq!(nanos.rounded().unwrap(), 43_200_000_000_000);
    }

    #[test]
    fn exact_integers_above_f64_precision_survive() {
        let mut nanos = ExactAccumulator::default();
        nanos
            .add(
                ExactNumber::Int(i128::from(i64::MAX)),
                super::super::NANOS_PER_SECOND,
            )
            .unwrap();
        assert_eq!(
            nanos.rounded().unwrap(),
            i128::from(i64::MAX) * 1_000_000_000
        );
    }
}
