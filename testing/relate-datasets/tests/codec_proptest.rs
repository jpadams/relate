//! Property suite for the compressed-adjacency codec
//! (`relate_datasets::codec`), following the storage crate's property
//! conventions (ADR-0268): one shared `Config::default()` so `PROPTEST_CASES`
//! and `PROPTEST_RNG_SEED` scale/seed every case, committed
//! `.proptest-regressions` for any failure proptest persists.
//!
//! Properties:
//!
//! 1. [`roundtrip_scalar`] — a random hop stream encodes and decodes back to
//!    the exact original, in order, under both schemes' scalar decoder.
//! 2. [`simd_matches_scalar`] — `decode_chunk_simd` is byte-identical to
//!    `decode_chunk` on the same encoded bytes (NEON on aarch64, the scalar
//!    fallback elsewhere — the cross-arch guard).
//! 3. [`ts_desc_order_preserved`] — a stream sorted by the engine's
//!    ts-DESC chunk comparator comes back in the same newest-first order.
//! 4. [`width_degeneracy_roundtrip`] — blocks forced to bit-widths 0, 1, 57,
//!    58, 63, and 64 (the SIMD window boundary sits between 57 and 58)
//!    roundtrip exactly.
//! 5. [`encode_is_deterministic`] — encoding the same stream twice yields
//!    identical bytes.

use proptest::prelude::*;
use proptest::test_runner::Config;
use relate_datasets::codec::{
    cmp_ts_desc, decode_chunk, decode_chunk_simd, encode_chunk, Hop, Scheme,
};

/// The suite-wide proptest configuration: `Config::default()`, which applies
/// the 256-case default and the `PROPTEST_*` environment overrides (the
/// nightly lane raises `PROPTEST_CASES`). Cases are cheap and in-memory, so
/// no fork or timeout override beyond proptest's own defaults.
fn config() -> Config {
    Config::default()
}

/// A timestamp field: `None` ~30%, otherwise an `i64` drawn adversarially —
/// the extremes (`MIN`, `MAX`, around zero) plus small dense and wide ranges,
/// so the gap stream hits every bit-width bucket including full 64-bit spans.
fn ts_value() -> impl Strategy<Value = Option<i64>> {
    prop_oneof![
        3 => Just(None),
        1 => prop::sample::select(vec![i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX])
            .prop_map(Some),
        3 => (0i64..=1024).prop_map(Some),
        3 => any::<i64>().prop_map(Some),
    ]
}

/// A relationship id: small dense values (FOR blocks at tiny widths), mid
/// ranges, and full u64-width bit patterns (forcing >32-bit block widths).
/// Duplicates are in-bounds for every branch.
fn id_value() -> impl Strategy<Value = i64> {
    prop_oneof![
        3 => 0i64..=64,
        2 => 0i64..=(1 << 20),
        1 => 0i64..=(1 << 40),
        2 => any::<u64>().prop_map(|v| v as i64),
    ]
}

/// One random hop stream, 0..=300 hops so a case spans several 128-value
/// FOR/ts blocks and 64-value hybrid blocks.
fn hop_stream() -> impl Strategy<Value = Vec<Hop>> {
    prop::collection::vec((ts_value(), id_value()), 0..=300)
        .prop_map(|pairs| pairs.into_iter().map(|(ts, id)| Hop { ts, id }).collect())
}

/// One of the two codec schemes.
fn scheme() -> impl Strategy<Value = Scheme> {
    prop::sample::select(Scheme::ALL.to_vec())
}

/// An id stream whose every 128-value FOR block and 64-value hybrid block
/// sits at (at least) the requested bit-width: values drawn from
/// `0..=max_for_width`, with the maximum itself injected periodically so the
/// block width cannot shrink below the target. Width 0 collapses to a single
/// repeated value.
fn width_forced_ids(width: u32) -> impl Strategy<Value = Vec<i64>> {
    let max: u64 = if width == 64 {
        u64::MAX
    } else if width == 0 {
        0
    } else {
        (1u64 << width) - 1
    };
    prop::collection::vec(any::<u64>(), 1..=300).prop_map(move |raw| {
        let mut ids: Vec<i64> = raw.iter().map(|&v| (v & max) as i64).collect();
        // Inject the width-defining maximum into every block so the block
        // bit-width is exactly the target (FOR: value - min; hybrid: the
        // zigzag delta from a 0 neighbour is at least as wide).
        for (i, id) in ids.iter_mut().enumerate() {
            if i % 16 == 0 {
                *id = max as i64;
            } else if width > 0 && i % 16 == 1 {
                *id = 0;
            }
        }
        ids
    })
}

proptest! {
    #![proptest_config(config())]

    /// Random hop streams roundtrip exactly, in order, through the scalar
    /// decoder of both schemes.
    #[test]
    fn roundtrip_scalar(hops in hop_stream(), scheme in scheme()) {
        let bytes = encode_chunk(scheme, &hops);
        prop_assert_eq!(&decode_chunk(scheme, &bytes), &hops, "{}", scheme.as_str());
    }

    /// The SIMD decode path is byte-identical to the scalar decode on the
    /// same encoded bytes. On aarch64 this exercises the NEON unpacker
    /// (including its >57-bit scalar fallback); elsewhere it is the scalar
    /// fallback and the property is the cross-arch guard.
    #[test]
    fn simd_matches_scalar(hops in hop_stream(), scheme in scheme()) {
        let bytes = encode_chunk(scheme, &hops);
        let scalar = decode_chunk(scheme, &bytes);
        let simd = decode_chunk_simd(scheme, &bytes);
        prop_assert_eq!(&simd, &scalar, "{}", scheme.as_str());
    }

    /// A ts-DESC-ordered stream (the engine's chunk invariant: missing ts
    /// first, then ts DESC, id DESC ties) roundtrips with its order
    /// unchanged — newest-first iteration is preserved byte-for-byte.
    #[test]
    fn ts_desc_order_preserved(hops in hop_stream(), scheme in scheme()) {
        let mut sorted = hops;
        sorted.sort_unstable_by(cmp_ts_desc);
        let bytes = encode_chunk(scheme, &sorted);
        prop_assert_eq!(&decode_chunk(scheme, &bytes), &sorted, "{}", scheme.as_str());
        prop_assert_eq!(&decode_chunk_simd(scheme, &bytes), &sorted, "{} SIMD", scheme.as_str());
    }

    /// Blocks pinned to the degenerate bit-widths — 0 (all-equal), 1, the
    /// SIMD window boundary 57/58, and 63/64 (near/full u64 span) —
    /// roundtrip exactly through both decoders and both schemes.
    #[test]
    fn width_degeneracy_roundtrip(
        ids in prop_oneof![
            width_forced_ids(0),
            width_forced_ids(1),
            width_forced_ids(57),
            width_forced_ids(58),
            width_forced_ids(63),
            width_forced_ids(64),
        ],
        scheme in scheme(),
    ) {
        let hops: Vec<Hop> = ids.into_iter().map(|id| Hop { ts: None, id }).collect();
        let bytes = encode_chunk(scheme, &hops);
        prop_assert_eq!(&decode_chunk(scheme, &bytes), &hops, "{}", scheme.as_str());
        prop_assert_eq!(&decode_chunk_simd(scheme, &bytes), &hops, "{} SIMD", scheme.as_str());
    }

    /// Encoding is a pure function of the stream: two encodes of the same
    /// hops produce identical bytes under both schemes.
    #[test]
    fn encode_is_deterministic(hops in hop_stream(), scheme in scheme()) {
        let first = encode_chunk(scheme, &hops);
        let second = encode_chunk(scheme, &hops);
        prop_assert_eq!(&first, &second, "{}", scheme.as_str());
    }
}
