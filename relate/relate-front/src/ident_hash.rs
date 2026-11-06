//! The one hasher every identifier-keyed front map and set hashes from
//! (ADR-0505 §4, the LABS-822 shape).
//!
//! Every scope map and set in `relate-front` is keyed by text the
//! query author wrote — variable names, projection aliases, property keys,
//! local callable names — so a fixed, publicly known hash function would
//! let one crafted statement pre-collide its identifiers and drive the
//! tables into quadratic probing; the 4 MiB query cap (ADR-0257) bounds
//! the input bytes, not that CPU work.
//!
//! The tables therefore hash with std's keyed SipHash-1-3 from **one
//! process-wide [`RandomState`]**, drawn once from the OS RNG on first use
//! — the shape `relate-executor`'s used-sets adopted in LABS-822
//! (`pattern_walk::shared_hasher`). Precomputation is defeated because
//! the 128-bit SipHash key is process-random and never derived from, or
//! reflected into, anything a query can observe: an attacker who cannot
//! learn the key cannot choose colliding identifiers offline, and SipHash
//! is a keyed PRF, so no key-independent collisions exist to fall back
//! on. One shared seed (instead of `RandomState::new()` per table) keeps
//! table construction free of the thread-local seed read the executor's
//! profile measured, and lets [`IdentBuildHasher`] be a zero-sized
//! `Default` type, so `IdentHashSet::default()` and
//! `with_capacity_and_hasher(n, IdentBuildHasher)` read exactly like the
//! FxHash spellings they replace.
//!
//! A seeded FxHash (`rustc_hash::FxSeededState`) was rejected: FxHasher's
//! `write` compresses a byte string through `hash_bytes` with fixed
//! constants *before* the seeded state is touched, so any two identifiers
//! that collide in that compression collide under every seed — the seed
//! cannot defeat precomputation for string keys.

use std::collections::hash_map::{DefaultHasher, RandomState};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::sync::OnceLock;

/// The process-wide SipHash key, drawn once from the OS RNG on first use.
static IDENT_HASHER: OnceLock<RandomState> = OnceLock::new();

/// The process-wide keyed state every identifier table hashes from.
fn shared_state() -> &'static RandomState {
    IDENT_HASHER.get_or_init(RandomState::new)
}

/// Zero-sized [`BuildHasher`] over the one process-wide [`RandomState`]:
/// every hasher it builds is SipHash-1-3 keyed with the shared seed.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IdentBuildHasher;

impl BuildHasher for IdentBuildHasher {
    type Hasher = DefaultHasher;

    #[inline]
    fn build_hasher(&self) -> DefaultHasher {
        shared_state().build_hasher()
    }
}

/// A set keyed by query-authored identifiers.
pub(crate) type IdentHashSet<T> = HashSet<T, IdentBuildHasher>;

/// A map keyed by query-authored identifiers.
pub(crate) type IdentHashMap<K, V> = HashMap<K, V, IdentBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The builder hashes through the one shared keyed state: a swap to an
    /// unkeyed or per-table hasher breaks this equality. (That the key
    /// differs between processes is not observable from inside one; it is
    /// `RandomState::new`'s OS-RNG contract, pinned here by type: the
    /// builder's hasher is std's keyed `DefaultHasher`.)
    #[test]
    fn hashes_from_the_one_process_wide_keyed_state() {
        let key = "attacker_chosen_identifier";
        assert_eq!(IdentBuildHasher.hash_one(key), shared_state().hash_one(key));
        // Two independently built tables agree: one seed, not one per map.
        let a: IdentHashSet<&str> = IdentHashSet::default();
        let b: IdentHashSet<&str> = IdentHashSet::with_capacity_and_hasher(4, IdentBuildHasher);
        assert_eq!(a.hasher().hash_one(key), b.hasher().hash_one(key));
        // A freshly keyed state draws a different key than the shared one.
        assert_ne!(
            RandomState::new().hash_one(key),
            IdentBuildHasher.hash_one(key)
        );
    }
}
