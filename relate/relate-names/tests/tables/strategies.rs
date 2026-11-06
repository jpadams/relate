//! The per-statement symbol-table strategies the `smallname_bench` example
//! measures (the spike's ADR-0509 corpus replay).
//!
//! One shared copy: this file is path-included by the bench
//! (`examples/smallname_bench.rs`) and by `tests/tables.rs`, which pins the
//! property the bench's figures rely on — every strategy assigns the same
//! first-seen ids. The strategies are bench apparatus, not engine API, so
//! they live here (test/example targets, rustc-hash as a dev-dependency)
//! rather than in `src/`.

use std::collections::HashMap;

use rustc_hash::FxHashMap;

use relate_names::SmallName;

/// The baseline: a std (SipHash) map keyed by owned `String`s — the
/// engine's current shape. Allocation happens once per distinct name
/// (interning semantics: probe by borrow, insert owned only on miss).
#[derive(Default)]
pub struct StringMapTable {
    map: HashMap<String, u32>,
}

impl StringMapTable {
    pub fn new() -> StringMapTable {
        StringMapTable::default()
    }

    pub fn lookup_or_insert(&mut self, name: &str) -> u32 {
        if let Some(id) = self.map.get(name) {
            return *id;
        }
        let id = self.map.len() as u32;
        self.map.insert(name.to_owned(), id);
        id
    }
}

/// The bench's control: the same owned-`String` baseline on the Fx
/// hasher — separates the representation win (SmallName keys) from the
/// hasher swap (SipHash → Fx). If this tracks the SmallName Fx table,
/// the win is the hasher; if it tracks the SipHash baseline, the
/// representation is doing the work.
#[derive(Default)]
pub struct StringFxTable {
    map: FxHashMap<String, u32>,
}

impl StringFxTable {
    pub fn new() -> StringFxTable {
        StringFxTable::default()
    }

    pub fn lookup_or_insert(&mut self, name: &str) -> u32 {
        if let Some(id) = self.map.get(name) {
            return *id;
        }
        let id = self.map.len() as u32;
        self.map.insert(name.to_owned(), id);
        id
    }
}

/// Strategy (b): a flat `Vec<(SmallName, u32)>` with linear scan — the
/// median-8 / p90-35 distinct-names-per-statement shape the corpus
/// measurement predicted.
#[derive(Default)]
pub struct SmallNameVecTable {
    entries: Vec<(SmallName, u32)>,
}

impl SmallNameVecTable {
    pub fn new() -> SmallNameVecTable {
        SmallNameVecTable::default()
    }

    /// Replay from the raw name text: classify, then table work.
    pub fn lookup_or_insert(&mut self, name: &str) -> u32 {
        self.lookup_or_insert_key(SmallName::from_name(name))
    }

    /// The wired-engine shape: the token already carries its `SmallName`,
    /// so lookup skips re-classification.
    pub fn lookup_or_insert_key(&mut self, probe: SmallName) -> u32 {
        if let Some(&(_, id)) = self.entries.iter().find(|(key, _)| *key == probe) {
            return id;
        }
        let id = self.entries.len() as u32;
        self.entries.push((probe, id));
        id
    }
}

/// Strategy (c): an `FxHashMap<SmallName, u32>` — the inline arm hashes a
/// bare `u64`, spilled arms hash their (short) text.
#[derive(Default)]
pub struct SmallNameFxTable {
    map: FxHashMap<SmallName, u32>,
}

impl SmallNameFxTable {
    pub fn new() -> SmallNameFxTable {
        SmallNameFxTable::default()
    }

    /// Replay from the raw name text: classify, then table work.
    pub fn lookup_or_insert(&mut self, name: &str) -> u32 {
        self.lookup_or_insert_key(SmallName::from_name(name))
    }

    /// The wired-engine shape: the token already carries its `SmallName`,
    /// so lookup skips re-classification.
    pub fn lookup_or_insert_key(&mut self, probe: SmallName) -> u32 {
        if let Some(id) = self.map.get(&probe) {
            return *id;
        }
        let id = self.map.len() as u32;
        self.map.insert(probe, id);
        id
    }
}
