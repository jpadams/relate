//! The bench's symbol-table strategies agree on first-seen id assignment.
//!
//! The strategies live in `tables/strategies.rs`, shared verbatim with the
//! `smallname_bench` example (path-included there, so there is exactly one
//! copy). This test promotes the spike's table-strategy check with the rest
//! of the suite: it pins the property the bench's figures rely on — every
//! strategy, raw-text or pre-classified, assigns the same first-seen ids.

#[path = "tables/strategies.rs"]
mod strategies;

use relate_names::SmallName;
use strategies::{SmallNameFxTable, SmallNameVecTable, StringFxTable, StringMapTable};

#[test]
fn all_table_strategies_assign_identical_first_seen_ids() {
    let occurrences = [
        "alpha", "beta", "alpha", "a b", "beta", "gamma", "a b", "alpha",
    ];
    let expected = [0, 1, 0, 2, 1, 3, 2, 0];
    let mut string_map = StringMapTable::new();
    let mut string_fx = StringFxTable::new();
    let mut vec_table = SmallNameVecTable::new();
    let mut fx_table = SmallNameFxTable::new();
    for (name, want) in occurrences.iter().zip(expected) {
        assert_eq!(
            string_map.lookup_or_insert(name),
            want,
            "string_map {name:?}"
        );
        assert_eq!(string_fx.lookup_or_insert(name), want, "string_fx {name:?}");
        assert_eq!(vec_table.lookup_or_insert(name), want, "vec table {name:?}");
        assert_eq!(fx_table.lookup_or_insert(name), want, "fx table {name:?}");
    }
    // the wired shape — keys already classified, no per-lookup
    // re-classification — assigns the same first-seen ids
    let mut vec_keyed = SmallNameVecTable::new();
    let mut fx_keyed = SmallNameFxTable::new();
    for (name, want) in occurrences.iter().zip(expected) {
        let key = SmallName::from_name(name);
        assert_eq!(
            vec_keyed.lookup_or_insert_key(key.clone()),
            want,
            "vec keyed {name:?}"
        );
        assert_eq!(
            fx_keyed.lookup_or_insert_key(key),
            want,
            "fx keyed {name:?}"
        );
    }
}
