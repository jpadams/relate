//! The IEEE-754 binary16 and bfloat16 bit encodings the FLOAT16/BFLOAT16
//! vector coordinate kinds narrow to (CIP-280, ADR-0464).

/// IEEE-754 binary16 encoding of an f32 with round-to-nearest-even. Only
/// used by [`crate::value::VectorCoordKind::narrow_float`]; infinity and
/// NaN keep their canonical half encodings, and a finite overflow encodes
/// as infinity so the caller's finiteness check reports it.
pub(crate) fn f32_to_f16_bits(f: f32) -> u16 {
    let bits = f.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let man = bits & 0x007f_ffff;
    let mut exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    if exp >= 0x1f {
        // Infinity, NaN, and finite overflow to infinity.
        let nan = (bits & 0x7fff_ffff) > 0x7f80_0000;
        return sign | 0x7c00 | (u16::from(nan) * 0x0200);
    }
    if exp <= 0 {
        if exp < -10 {
            return sign; // underflows to zero
        }
        // Subnormal: the implicit bit joins the mantissa, then the run is
        // right-shifted into the 10-bit subnormal field with RNE.
        let shift = (14 - exp) as u32;
        let full = man | 0x0080_0000;
        let mut out = (full >> shift) as u16;
        let rem = full & ((1 << shift) - 1);
        let half = 1u32 << (shift - 1);
        if rem > half || (rem == half && out & 1 == 1) {
            out += 1;
        }
        return sign | out;
    }
    let mut half_man = (man >> 13) as u16;
    let rem = man & 0x1fff;
    if rem > 0x1000 || (rem == 0x1000 && half_man & 1 == 1) {
        half_man += 1;
        if half_man == 0x0400 {
            // The mantissa carry rounds into the next exponent.
            half_man = 0;
            exp += 1;
            if exp >= 0x1f {
                return sign | 0x7c00;
            }
        }
    }
    sign | ((exp as u16) << 10) | half_man
}

/// Widening of an IEEE-754 binary16 bit pattern back to f32, subnormals
/// included.
pub(crate) fn f16_bits_to_f32(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let man = u32::from(h & 0x03ff);
    let exp = u32::from((h >> 10) & 0x1f);
    let bits = if exp == 0 {
        if man == 0 {
            sign
        } else {
            // Normalise the subnormal: half subnormals are `man * 2^-24`.
            let mut m = man;
            let mut e = 113;
            while m & 0x0400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((m & 0x03ff) << 13)
        }
    } else if exp == 0x1f {
        sign | 0x7f80_0000 | (man << 13)
    } else {
        sign | ((exp + 112) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}

/// bfloat16 encoding of an f32 with round-to-nearest-even: the top 16 bits
/// of the binary32 pattern. NaN payloads stay NaN.
pub(crate) fn bfloat16_bits(f: f32) -> u32 {
    let bits = f.to_bits();
    if (bits & 0x7fff_ffff) > 0x7f80_0000 {
        return bits | 0x0000_ffff;
    }
    let lsb = (bits >> 16) & 1;
    bits.wrapping_add(0x7fff + lsb) & 0xffff_0000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_to_f16_bits_encodes_the_common_values() {
        assert_eq!(f32_to_f16_bits(0.0), 0x0000);
        assert_eq!(f32_to_f16_bits(-0.0), 0x8000);
        assert_eq!(f32_to_f16_bits(1.0), 0x3c00);
        assert_eq!(f32_to_f16_bits(-2.0), 0xc000);
        assert_eq!(f32_to_f16_bits(0.1), 0x2e66);
        assert_eq!(f32_to_f16_bits(65504.0), 0x7bff);
    }

    #[test]
    fn f32_to_f16_bits_encodes_infinities_nan_and_overflow() {
        assert_eq!(f32_to_f16_bits(f32::INFINITY), 0x7c00);
        assert_eq!(f32_to_f16_bits(f32::NEG_INFINITY), 0xfc00);
        // A finite overflow encodes as infinity so the caller's finiteness
        // check reports it.
        assert_eq!(f32_to_f16_bits(65520.0), 0x7c00);
        assert_eq!(f32_to_f16_bits(-65520.0), 0xfc00);
        // NaN keeps the quiet-bit encoding; the sign survives.
        assert_eq!(f32_to_f16_bits(f32::NAN), 0x7e00);
        assert_eq!(f32_to_f16_bits(-f32::NAN), 0xfe00);
    }

    #[test]
    fn f32_to_f16_bits_rounds_subnormals_and_ties_to_even() {
        // The smallest half subnormal is 2^-24; the next one down is the
        // tie that RNE sends to zero.
        assert_eq!(f32_to_f16_bits(2f32.powi(-24)), 0x0001);
        assert_eq!(f32_to_f16_bits(2f32.powi(-25)), 0x0000);
        // 1 + 2^-11 sits exactly between 1.0 and 1 + 2^-10; RNE keeps the
        // even mantissa. 1 + 3*2^-11 rounds up to the even one above.
        assert_eq!(f32_to_f16_bits(1.0 + 2f32.powi(-11)), 0x3c00);
        assert_eq!(f32_to_f16_bits(1.0 + 3.0 * 2f32.powi(-11)), 0x3c02);
        // 2 - 2^-11 sits exactly between 0x3fff and 2.0; rounding up
        // carries the top mantissa into the next exponent.
        assert_eq!(f32_to_f16_bits(2.0 - 2f32.powi(-11)), 0x4000);
    }

    #[test]
    fn f16_bits_to_f32_widens_normals_subnormals_and_specials() {
        assert_eq!(f16_bits_to_f32(0x0000), 0.0);
        assert_eq!(f16_bits_to_f32(0x8000).to_bits(), (-0.0f32).to_bits());
        assert_eq!(f16_bits_to_f32(0x3c00), 1.0);
        assert_eq!(f16_bits_to_f32(0xc000), -2.0);
        assert_eq!(f16_bits_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(f16_bits_to_f32(0x03ff), 1023.0 * 2f32.powi(-24));
        assert_eq!(f16_bits_to_f32(0x7bff), 65504.0);
        assert_eq!(f16_bits_to_f32(0x7c00), f32::INFINITY);
        assert_eq!(f16_bits_to_f32(0xfc00), f32::NEG_INFINITY);
        assert!(f16_bits_to_f32(0x7e00).is_nan());
    }

    #[test]
    fn f16_bits_round_trip_through_the_widening() {
        for half in [0x0000_u16, 0x0001, 0x03ff, 0x0400, 0x3c00, 0x7bff, 0xbc00] {
            assert_eq!(f32_to_f16_bits(f16_bits_to_f32(half)), half);
        }
    }

    #[test]
    fn bfloat16_bits_round_to_nearest_even() {
        assert_eq!(bfloat16_bits(1.0), 0x3f80_0000);
        assert_eq!(bfloat16_bits(-2.0), 0xc000_0000);
        // 257.0 sits exactly halfway between 256.0 and 258.0; RNE picks
        // the even mantissa.
        assert_eq!(bfloat16_bits(257.0), 0x4380_0000);
        // Halfway with an odd mantissa rounds up instead.
        assert_eq!(bfloat16_bits(1.0 + 3.0 * 2f32.powi(-8)), 0x3f82_0000);
    }

    #[test]
    fn bfloat16_bits_preserve_nan_and_encode_subnormals() {
        let nan = bfloat16_bits(f32::NAN);
        assert_eq!(nan & 0x7f80_0000, 0x7f80_0000);
        assert_ne!(nan & 0x007f_ffff, 0);
        assert_eq!(bfloat16_bits(f32::INFINITY), 0x7f80_0000);
        assert_eq!(bfloat16_bits(f32::NEG_INFINITY), 0xff80_0000);
        // 2^-133 is a bfloat16 subnormal: the top half of the f32 pattern.
        assert_eq!(bfloat16_bits(2f32.powi(-133)), 0x0001_0000);
    }
}
