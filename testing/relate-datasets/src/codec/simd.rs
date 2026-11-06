//! SIMD bit unpacking: fixed-width block -> u64 values, no bit-reader state
//! machine. aarch64 uses NEON per-lane variable shifts; other targets fall
//! back to the scalar reader so the crate compiles everywhere.

use super::bitpack::BitReader;
#[cfg(target_arch = "aarch64")]
use super::FOR_BLOCK;

/// True when the NEON decode path is compiled in (aarch64 only).
pub fn simd_available() -> bool {
    cfg!(target_arch = "aarch64")
}

/// Largest width a value fits one 8-byte window: `bit_offset % 8` is at most
/// 7, so `7 + width <= 64`. Wider blocks take the scalar reader.
#[cfg(target_arch = "aarch64")]
const WINDOW_MAX_WIDTH: u32 = 57;

/// Padded copy buffer: the largest packed block is `128 values x 64 bits`
/// plus the 8 bytes a window may read past the block end.
#[cfg(target_arch = "aarch64")]
const PAD_BUF: usize = (FOR_BLOCK * 64) / 8 + 8;

/// Scalar decode entry point: append `count` values of `width` bits from
/// `packed` through the bit reader.
pub(crate) fn unpack_block_scalar(packed: &[u8], width: u32, count: usize, out: &mut Vec<u64>) {
    let mut r = BitReader::new(packed);
    for _ in 0..count {
        out.push(r.read(width));
    }
}

/// Append `count` values of `width` bits from `packed` via the NEON path:
/// two values per step, each from its own unaligned 8-byte window assembled
/// into one `u64x2`, right-shifted per lane (`vshlq_u64` with negative
/// counts) and masked in parallel.
///
/// # Safety
///
/// NEON is architecturally mandatory on aarch64; only called under
/// `cfg(target_arch = "aarch64")`. The caller must guarantee:
///
/// - `width` is in `1..=WINDOW_MAX_WIDTH`, so each value fits a single
///   8-byte window and the `(1 << width) - 1` mask cannot overflow;
/// - `packed.len() <= PAD_BUF`, so the padded copy buffer holds the block.
///
/// Both hold for every chunk block [`super::decode_chunk`] produces
/// (at most `FOR_BLOCK` values at up to 64 bits), enforced by
/// [`unpack_block_simd`].
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn unpack_block_neon(packed: &[u8], width: u32, count: usize, out: &mut Vec<u64>) {
    use std::arch::aarch64::{
        vandq_u64, vcombine_s64, vcombine_u64, vcreate_s64, vcreate_u64, vdupq_n_u64, vshlq_u64,
        vst1q_u64,
    };
    debug_assert!((1..=WINDOW_MAX_WIDTH).contains(&width));
    debug_assert!(packed.len() <= PAD_BUF);
    let mut buf = [0u8; PAD_BUF];
    buf[..packed.len()].copy_from_slice(packed);
    let w = width as usize;
    let vmask = vdupq_n_u64((1u64 << width) - 1);
    let mut i = 0;
    while i + 2 <= count {
        let b0 = i * w / 8;
        let b1 = (i + 1) * w / 8;
        let w0 = u64::from_le_bytes(buf[b0..b0 + 8].try_into().expect("window"));
        let w1 = u64::from_le_bytes(buf[b1..b1 + 8].try_into().expect("window"));
        let v = vcombine_u64(vcreate_u64(w0), vcreate_u64(w1));
        let shifts = vcombine_s64(
            vcreate_s64((-((i * w % 8) as i64)) as u64),
            vcreate_s64((-(((i + 1) * w % 8) as i64)) as u64),
        );
        let masked = vandq_u64(vshlq_u64(v, shifts), vmask);
        let mut pair = [0u64; 2];
        // SAFETY: `pair` is a valid two-lane u64 store target.
        unsafe { vst1q_u64(pair.as_mut_ptr(), masked) };
        out.extend_from_slice(&pair);
        i += 2;
    }
    if i < count {
        let b = i * w / 8;
        let win = u64::from_le_bytes(buf[b..b + 8].try_into().expect("window"));
        out.push((win >> (i * w % 8)) & ((1u64 << width) - 1));
    }
}

/// aarch64 SIMD entry point: width-0 blocks are all zeros (the NEON path is
/// contractually `width >= 1`, so zero-width blocks are answered here),
/// blocks wider than one 8-byte window fall back to the scalar reader,
/// everything else takes the NEON path.
#[cfg(target_arch = "aarch64")]
pub(crate) fn unpack_block_simd(packed: &[u8], width: u32, count: usize, out: &mut Vec<u64>) {
    if width == 0 {
        out.extend(std::iter::repeat_n(0, count));
        return;
    }
    if width > WINDOW_MAX_WIDTH {
        return unpack_block_scalar(packed, width, count, out);
    }
    // SAFETY: NEON is always available on aarch64; width is in
    // `1..=WINDOW_MAX_WIDTH` and chunk blocks never exceed `PAD_BUF` bytes.
    unsafe { unpack_block_neon(packed, width, count, out) }
}

/// Non-aarch64 build: the SIMD entry point falls back to the scalar reader,
/// so x86_64 compiles and measures the identical scalar numbers.
#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn unpack_block_simd(packed: &[u8], width: u32, count: usize, out: &mut Vec<u64>) {
    unpack_block_scalar(packed, width, count, out);
}
