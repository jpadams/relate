//! The frontier family's sparse node-keyed map (ADR-0478 Decision 6's
//! allocation half, the sparse layout): an open-addressed, linearly
//! probed table from [`NodeId`] to a per-node value whose memory is
//! proportional to the nodes a run TOUCHED, never to the store's
//! node-id span, and whose reset is O(1) — a generation bump, never a
//! sweep.
//!
//! **The generation stamp.** Every occupied slot carries the generation
//! that wrote it; a slot counts as live only under the map's current
//! generation. [`NodeMap::clear`] bumps the generation, so every entry
//! of the previous generation goes stale at once — exactly the sparse
//! set's constant-time clear (a stale slot reads as empty to every
//! probe), without the span-sized `sparse` array a classic two-array
//! sparse set needs. A stale slot's value is dropped when a later
//! insertion reuses the slot, or when the map grows (the rehash carries
//! live entries only) or drops. The generation counter wraps once per
//! 2³² clears; the wrap is the one sweep — every slot emptied, the
//! counter restarted.
//!
//! **Probing.** Linear probing from a Fibonacci hash of the node id
//! (node ids are allocator-issued and dense, so the multiplicative hash
//! spreads consecutive ids across the table). No single key is ever
//! removed within a generation — the frontier's tables only grow
//! between resets — so a probe chain never holds a hole, and a probe
//! stops at the first non-live slot.
//!
//! **The size.** A fresh map holds no slots at all; the first insertion
//! allocates [`MIN_SLOTS`], and the table doubles whenever an insertion
//! would cross the load ceiling ([`LOAD_NUM`]/[`LOAD_DEN`] of the slot
//! count). [`NodeMap::heap_bytes`] is the exact slot-array allocation —
//! the frontier family's table ceiling reads it, never an estimate.

use std::mem::size_of;
use std::num::NonZeroU32;

use relate_core::NodeId;

/// The slot count of a map's first allocation — a power of two, small
/// enough that a run touching a handful of nodes allocates a few
/// hundred bytes.
pub(crate) const MIN_SLOTS: usize = 16;

/// The load ceiling's numerator: live entries never exceed
/// `slots × LOAD_NUM / LOAD_DEN` — linear probing's probe lengths stay
/// short below three quarters full.
const LOAD_NUM: usize = 3;

/// The load ceiling's denominator (see [`LOAD_NUM`]).
const LOAD_DEN: usize = 4;

/// The 64-bit Fibonacci multiplier (2⁶⁴ / φ, odd) — the multiplicative
/// hash's one constant.
const FIBONACCI: u64 = 0x9E37_79B9_7F4A_7C15;

/// One occupied slot: the generation that wrote it, the key, the value.
/// The stamp is non-zero, so an empty slot (`None`) costs no tag byte —
/// a slot is exactly an entry's size.
#[derive(Debug)]
struct Entry<V> {
    /// The generation this entry was written under — live only while it
    /// equals the map's current generation.
    stamp: NonZeroU32,
    /// The key.
    node: NodeId,
    /// The per-node value.
    value: V,
}

/// A sparse map from node id to `V`, sized by the keys inserted since
/// the last [`Self::clear`] and reset in O(1) — see the module docs for
/// the layout and the reset.
#[derive(Debug)]
pub(crate) struct NodeMap<V> {
    /// The slot array, empty or a power of two long. `None` is a slot
    /// never written since the last growth; `Some` under a stale stamp
    /// is a previous generation's entry — both read as empty.
    slots: Vec<Option<Entry<V>>>,
    /// The live entries under the current generation.
    live: usize,
    /// The current generation, starting at 1.
    generation: NonZeroU32,
}

impl<V> Default for NodeMap<V> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            live: 0,
            generation: NonZeroU32::MIN,
        }
    }
}

impl<V> NodeMap<V> {
    /// Forget every entry in O(1): bump the generation, every current
    /// entry stale at once. The slot array is kept — the next
    /// generation reuses it without allocating. The wrap (once per 2³²
    /// clears) is the one sweep: every slot emptied, the counter
    /// restarted at 1.
    pub(crate) fn clear(&mut self) {
        self.live = 0;
        match self.generation.checked_add(1) {
            Some(next) => self.generation = next,
            None => {
                for slot in &mut self.slots {
                    *slot = None;
                }
                self.generation = NonZeroU32::MIN;
            }
        }
    }

    /// The live entry count — the keys inserted since the last clear. A
    /// test-only accessor (the reset and sizing pins read it).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.live
    }

    /// The slot array's exact allocation in bytes — the table ceiling's
    /// accounting read (stale slots included: they are allocated memory
    /// the next generation reuses).
    pub(crate) fn heap_bytes(&self) -> usize {
        self.slots.capacity() * size_of::<Option<Entry<V>>>()
    }

    /// The live value under `node`, if any.
    pub(crate) fn get(&self, node: NodeId) -> Option<&V> {
        match self.probe(node) {
            Probe::Live(index) => self.slots[index].as_ref().map(|entry| &entry.value),
            Probe::Vacant(_) | Probe::Unallocated => None,
        }
    }

    /// The live value under `node`, mutably, if any.
    pub(crate) fn get_mut(&mut self, node: NodeId) -> Option<&mut V> {
        match self.probe(node) {
            Probe::Live(index) => self.slots[index].as_mut().map(|entry| &mut entry.value),
            Probe::Vacant(_) | Probe::Unallocated => None,
        }
    }

    /// The live value under `node`, inserting `make()` first when the
    /// node holds none — answering whether the insertion happened (the
    /// callers' check-and-set: a visit set reads the flag, a record
    /// table ignores it).
    pub(crate) fn touch(&mut self, node: NodeId, make: impl FnOnce() -> V) -> (&mut V, bool) {
        let (index, inserted) = match self.probe(node) {
            Probe::Live(index) => (index, false),
            Probe::Vacant(_) | Probe::Unallocated => {
                if (self.live + 1) * LOAD_DEN > self.slots.len() * LOAD_NUM {
                    self.grow();
                }
                let Probe::Vacant(index) = self.probe(node) else {
                    unreachable!("a key absent before the growth is absent after it")
                };
                self.slots[index] = Some(Entry {
                    stamp: self.generation,
                    node,
                    value: make(),
                });
                self.live += 1;
                (index, true)
            }
        };
        let entry = self.slots[index]
            .as_mut()
            .expect("the probed or just-written slot is occupied");
        (&mut entry.value, inserted)
    }

    /// Set `node`'s value, replacing a live one.
    pub(crate) fn insert(&mut self, node: NodeId, value: V) {
        match self.get_mut(node) {
            Some(slot) => *slot = value,
            None => {
                self.touch(node, || value);
            }
        }
    }

    /// Whether `slot` holds a current-generation entry.
    fn is_live(&self, slot: &Option<Entry<V>>) -> bool {
        matches!(slot, Some(entry) if entry.stamp == self.generation)
    }

    /// `node`'s home slot: the Fibonacci hash's top bits, as many as the
    /// slot count's power of two. The caller guarantees a non-empty
    /// table.
    fn home(&self, node: NodeId) -> usize {
        let bits = self.slots.len().trailing_zeros();
        ((node as u64).wrapping_mul(FIBONACCI) >> (64 - bits)) as usize
    }

    /// The linear probe for `node`: its live slot, or the first non-live
    /// slot on its chain (where an insertion lands), or no table at all.
    fn probe(&self, node: NodeId) -> Probe {
        if self.slots.is_empty() {
            return Probe::Unallocated;
        }
        let mask = self.slots.len() - 1;
        let mut index = self.home(node);
        loop {
            let slot = &self.slots[index];
            if !self.is_live(slot) {
                return Probe::Vacant(index);
            }
            if slot.as_ref().is_some_and(|entry| entry.node == node) {
                return Probe::Live(index);
            }
            index = (index + 1) & mask;
        }
    }

    /// Double the slot array (or allocate the first [`MIN_SLOTS`]),
    /// rehashing the live entries — stale entries are dropped here, the
    /// growth's own garbage collection.
    fn grow(&mut self) {
        let len = (self.slots.len() * 2).max(MIN_SLOTS);
        let mut slots = Vec::with_capacity(len);
        slots.resize_with(len, || None);
        let old = std::mem::replace(&mut self.slots, slots);
        let generation = self.generation;
        for entry in old.into_iter().flatten() {
            if entry.stamp != generation {
                continue;
            }
            let Probe::Vacant(index) = self.probe(entry.node) else {
                unreachable!("live keys are distinct, and the grown table has room")
            };
            self.slots[index] = Some(entry);
        }
    }
}

/// One probe's answer.
enum Probe {
    /// The key's live slot.
    Live(usize),
    /// The key is absent; an insertion lands at this slot.
    Vacant(usize),
    /// The map holds no slot array yet.
    Unallocated,
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{NodeMap, MIN_SLOTS};

    /// Insert, read back, overwrite, and the check-and-set flag.
    #[test]
    fn touch_inserts_once_and_insert_overwrites() {
        let mut map = NodeMap::<u32>::default();
        assert_eq!(map.get(5), None);
        let (value, inserted) = map.touch(5, || 1);
        assert!(inserted);
        *value += 1;
        let (value, inserted) = map.touch(5, || 99);
        assert!(!inserted);
        assert_eq!(*value, 2);
        map.insert(5, 7);
        map.insert(6, 8);
        assert_eq!(map.get(5), Some(&7));
        assert_eq!(map.get(6), Some(&8));
        assert_eq!(map.len(), 2);
    }

    /// The O(1) clear: every entry of the previous generation reads
    /// absent, its slot reusable, the slot array kept.
    #[test]
    fn clear_retires_every_entry_without_freeing_the_slots() {
        let mut map = NodeMap::<u32>::default();
        for node in 0..10 {
            map.insert(node, node as u32);
        }
        let bytes = map.heap_bytes();
        map.clear();
        assert_eq!(map.len(), 0);
        assert_eq!(map.heap_bytes(), bytes);
        for node in 0..10 {
            assert_eq!(map.get(node), None, "{node} is stale after the clear");
        }
        let (value, inserted) = map.touch(3, || 33);
        assert!(inserted, "a stale key inserts afresh");
        assert_eq!(*value, 33);
    }

    /// Growth rehashes the live entries only, and keeps every one; the
    /// table never grows before it holds keys.
    #[test]
    fn growth_keeps_every_live_entry_and_drops_stale_ones() {
        let mut map = NodeMap::<i64>::default();
        assert_eq!(map.heap_bytes(), 0);
        map.insert(-1, -1);
        let first = map.heap_bytes();
        assert!(first > 0);
        map.clear();
        let keys: Vec<i64> = (0..10_000).map(|i| i * 7_919 + (1 << 40)).collect();
        for &key in &keys {
            map.insert(key, key);
        }
        assert_eq!(map.len(), keys.len());
        assert_eq!(map.get(-1), None, "the stale entry did not survive growth");
        for &key in &keys {
            assert_eq!(map.get(key), Some(&key));
        }
        assert!(map.heap_bytes() >= first * keys.len() / MIN_SLOTS);
    }

    /// The generation counter's wrap is the one sweep: entries written
    /// just before the wrap never read live after it.
    #[test]
    fn the_generation_wrap_empties_every_slot() {
        let mut map = NodeMap::<u32> {
            generation: NonZeroU32::new(u32::MAX - 1).expect("non-zero"),
            ..NodeMap::default()
        };
        map.insert(1, 1);
        map.clear();
        map.insert(2, 2);
        assert_eq!(map.generation.get(), u32::MAX);
        map.clear();
        assert_eq!(map.generation, NonZeroU32::MIN);
        assert_eq!(map.get(1), None);
        assert_eq!(map.get(2), None);
        map.insert(1, 10);
        assert_eq!(map.get(1), Some(&10));
    }
}
