//! A 128-bit structural hash of a [`Value`], and the memo table keyed by it
//! (LABS-1478, memo tier 3: a wide-value key).
//!
//! **Why no verification.** A memo hit is a wrong query result if two
//! different values share a key, so the key must be collision-free in
//! practice. A full-content 128-bit hash is: random collisions are about
//! 2^-128 per pair, far below hardware error rates, so a hit is one probe
//! with no equality walk. (A *sampled* hash cannot make that claim, its
//! collisions are structured, and is deliberately not offered.) The cost is
//! one pass over the value, O(size), paid once per distinct value the memo
//! sees; a key that repeats is paid once per lookup only by this hash, so a
//! caller holding a value across many lookups should keep its [`Hash128`]
//! (the companion hash). Caching the hash inside the shared container
//! itself needs a change to `Value`'s representation in `relate-core` and
//! is a separate decision.
//!
//! **Not cryptographic.** Scalars are folded into two 64-bit lanes with
//! 64x64->128 multiplies; strings and byte runs go through XXH3-128
//! (`xxhash-rust`, no dependencies), which runs at tens of GB/s where this
//! module's own word loop managed 3. Both are keyed with a per-process
//! random seed, so a client that controls the values cannot craft a
//! collision without it; neither is designed against a determined
//! adversary.
//!
//! **What hashes.** The representation, tagged by variant: a value equal
//! under the engine's `=` but spelled differently (`1` and `1.0`) gets two
//! keys, which costs a missed hit and never a wrong one. Entities (node,
//! relationship, path), temporals, points and vectors return `None` (the
//! memo bypasses): an entity is keyed by its id in the id tables, not by
//! content. Nesting deeper than [`MAX_DEPTH`] returns `None`.

#![allow(
    dead_code,
    reason = "memo tier 3: the first consumer is a value-keyed expression memo (LABS-1478 follow-up); pinned by the tests below"
)]

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::OnceLock;

use relate_core::{NodeId, Value};

use super::memo::MemoTable;
use super::node_map::NodeMap;

/// Nesting deeper than this is not hashed (the memo bypasses).
const MAX_DEPTH: u32 = 24;

const K0: u64 = 0xa076_1d64_78bd_642f;
const K1: u64 = 0xe703_7ed1_a0b4_28db;
const K2: u64 = 0x8ebc_6af0_9c88_c6e3;
const K3: u64 = 0x5899_65cc_7537_4cc3;

/// A value's 128-bit content key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Hash128(u128);

fn seed() -> (u64, u64) {
    static SEED: OnceLock<(u64, u64)> = OnceLock::new();
    *SEED.get_or_init(|| {
        let state = RandomState::new();
        (state.hash_one(0x5eed_u64), state.hash_one(0xfeed_u64))
    })
}

/// The folded multiply: the high and low halves of the product, xored.
#[inline]
fn fold(x: u64, y: u64) -> u64 {
    let product = u128::from(x).wrapping_mul(u128::from(y));
    (product as u64) ^ ((product >> 64) as u64)
}

struct Lanes {
    a: u64,
    b: u64,
    /// The per-process seed for the byte runs' XXH3.
    seed: u64,
}

impl Lanes {
    fn new() -> Self {
        let (a, b) = seed();
        Self {
            a,
            b,
            seed: a ^ b.rotate_left(32),
        }
    }

    #[inline]
    fn word(&mut self, w: u64) {
        self.a = fold(self.a ^ w ^ K0, K1 ^ self.b.rotate_left(17));
        self.b = fold(self.b ^ w.rotate_left(32) ^ K2, K3 ^ self.a.rotate_left(29));
    }

    /// A byte run: its length, then its XXH3-128 (seeded per process, SIMD
    /// where the target has it, 20-50 GB/s on large runs) folded in as two
    /// words. Each run is hashed on its own, so `("ab","c")` and
    /// `("a","bc")` differ.
    fn bytes(&mut self, bytes: &[u8]) {
        self.word(bytes.len() as u64);
        let digest = xxhash_rust::xxh3::xxh3_128_with_seed(bytes, self.seed);
        self.word(digest as u64);
        self.word((digest >> 64) as u64);
    }

    fn finish(self) -> Hash128 {
        let hi = fold(self.a ^ K0, K1);
        let lo = fold(self.b ^ K2, K3);
        Hash128((u128::from(hi) << 64) | u128::from(lo))
    }
}

fn walk(lanes: &mut Lanes, value: &Value, depth: u32) -> Option<()> {
    if depth > MAX_DEPTH {
        return None;
    }
    match value {
        Value::Null => lanes.word(0),
        Value::Bool(b) => {
            lanes.word(1);
            lanes.word(u64::from(*b));
        }
        Value::Int(i) => {
            lanes.word(2);
            lanes.word(*i as u64);
        }
        Value::Float(f) => {
            lanes.word(3);
            lanes.word(f.to_bits());
        }
        Value::String(s) => {
            lanes.word(4);
            lanes.bytes(s.as_bytes());
        }
        Value::List(items) => {
            lanes.word(5);
            lanes.word(items.len() as u64);
            for item in items.iter() {
                walk(lanes, item, depth + 1)?;
            }
        }
        Value::Map(entries) => {
            lanes.word(6);
            lanes.word(entries.len() as u64);
            for (key, item) in entries.iter() {
                lanes.bytes(key.as_bytes());
                walk(lanes, item, depth + 1)?;
            }
        }
        Value::Uuid(u) => {
            lanes.word(7);
            lanes.word(*u as u64);
            lanes.word((*u >> 64) as u64);
        }
        Value::Bytes(b) => {
            lanes.word(8);
            lanes.bytes(b);
        }
        Value::Node(_)
        | Value::Relationship(_)
        | Value::Path(_)
        | Value::Temporal(_)
        | Value::Point(_)
        | Value::Vector { .. } => return None,
    }
    Some(())
}

/// `value`'s content key, `None` for a value the memo bypasses.
pub(crate) fn hash_value(value: &Value) -> Option<Hash128> {
    let mut lanes = Lanes::new();
    walk(&mut lanes, value, 0)?;
    Some(lanes.finish())
}

/// Any `Copy` value per [`Hash128`] key, in the sparse node map: the low 64
/// bits index it and the high 64 are stored beside the value, so a hit
/// matches all 128 bits and a low-half clash is a miss, never a wrong hit.
pub(crate) struct HashedValues<V>(NodeMap<(u64, V)>);

impl<V> Default for HashedValues<V> {
    fn default() -> Self {
        Self(NodeMap::default())
    }
}

impl<V: Copy> MemoTable for HashedValues<V> {
    type Key = Hash128;
    type Value = V;

    fn get(&self, key: Hash128) -> Option<V> {
        let (hi, lo) = ((key.0 >> 64) as u64, key.0 as u64);
        match self.0.get(lo as NodeId) {
            Some(&(stored, value)) if stored == hi => Some(value),
            _ => None,
        }
    }

    fn insert(&mut self, key: Hash128, value: V) {
        let (hi, lo) = ((key.0 >> 64) as u64, key.0 as u64);
        self.0.insert(lo as NodeId, (hi, value));
    }

    fn heap_bytes(&self) -> usize {
        self.0.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Arc;

    use super::super::memo::{Memo, MemoMeter, MemoPolicy};
    use super::*;

    fn s(text: &str) -> Value {
        Value::String(Arc::from(text))
    }

    fn list(items: Vec<Value>) -> Value {
        Value::List(Arc::new(items))
    }

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(Arc::new(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
        ))
    }

    fn h(value: &Value) -> Hash128 {
        hash_value(value).expect("hashable")
    }

    struct Unmetered;
    impl MemoMeter for Unmetered {
        fn charge(&self, _: &mut u64, _: u64) -> relate_core::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn equal_values_hash_equal_and_variants_stay_apart() {
        assert_eq!(h(&s("abc")), h(&s("abc")));
        assert_eq!(
            h(&list(vec![Value::Int(1), s("x")])),
            h(&list(vec![Value::Int(1), s("x")]))
        );
        let keys: HashSet<_> = [
            Value::Null,
            Value::Bool(true),
            Value::Bool(false),
            Value::Int(1),
            Value::Int(0),
            Value::Float(1.0),
            s("1"),
            s(""),
            list(vec![]),
            map(&[]),
            Value::Uuid(1),
            Value::Bytes(Arc::from(&b"1"[..])),
        ]
        .iter()
        .map(h)
        .collect();
        assert_eq!(keys.len(), 12, "each of the twelve is its own key");
    }

    #[test]
    fn structure_boundaries_matter() {
        assert_ne!(
            h(&list(vec![
                list(vec![Value::Int(1)]),
                list(vec![Value::Int(2)])
            ])),
            h(&list(vec![list(vec![Value::Int(1), Value::Int(2)])]))
        );
        assert_ne!(
            h(&list(vec![s("ab"), s("c")])),
            h(&list(vec![s("a"), s("bc")]))
        );
        assert_ne!(h(&map(&[("ab", s("c"))])), h(&map(&[("a", s("bc"))])));
        assert_ne!(
            h(&list(vec![Value::Int(1), Value::Int(2)])),
            h(&list(vec![Value::Int(2), Value::Int(1)]))
        );
    }

    #[test]
    fn long_values_differing_in_the_middle_do_not_collide() {
        let base = "x".repeat(10_000);
        let mut other = base.clone().into_bytes();
        other[5_000] = b'y';
        let other = String::from_utf8(other).unwrap();
        assert_ne!(h(&s(&base)), h(&s(&other)));
    }

    #[test]
    fn no_collisions_over_many_distinct_values() {
        let mut seen = HashSet::new();
        for i in 0..100_000i64 {
            assert!(seen.insert(h(&Value::Int(i))), "int {i}");
            assert!(seen.insert(h(&s(&format!("k{i}")))), "string {i}");
        }
    }

    #[test]
    fn entities_and_over_deep_nesting_bypass() {
        let mut deep = Value::Int(0);
        for _ in 0..(MAX_DEPTH + 2) {
            deep = list(vec![deep]);
        }
        assert!(hash_value(&deep).is_none());
        let shallow = list(vec![list(vec![Value::Int(0)])]);
        assert!(hash_value(&shallow).is_some());
    }

    #[test]
    fn the_table_matches_all_128_bits() {
        let mut table = HashedValues::<u32>::default();
        let key = h(&s("alpha"));
        table.insert(key, 7);
        assert_eq!(table.get(key), Some(7));
        assert_eq!(table.get(h(&s("beta"))), None);
        // Same low half, different high half: a miss, not a wrong hit.
        let clash = Hash128((u128::from(!(key.0 >> 64) as u64) << 64) | (key.0 & u64::MAX as u128));
        assert_eq!(table.get(clash), None);
    }

    #[test]
    fn a_memo_over_value_keys_computes_once_per_distinct_value() {
        let policy = MemoPolicy {
            warm_up: 0,
            ..MemoPolicy::MODERATE
        };
        let mut memo = Memo::<HashedValues<u64>>::new(policy, None);
        let blob = map(&[("a", list(vec![Value::Int(1), s("two")]))]);
        let mut calls = 0;
        for _ in 0..5 {
            let got = memo
                .answer(&Unmetered, h(&blob), || {
                    calls += 1;
                    Ok(42)
                })
                .unwrap();
            assert_eq!(got, 42);
        }
        assert_eq!(calls, 1);
    }
}
