//! Multiset equality of Cypher name lists.
//!
//! This is the lowest shared home of the policy: the parser's EXISTS/COUNT
//! UNION screening, the front's validator, the planner's output-name
//! computation and the executor's UNION row alignment all compare column-name
//! lists through [`same_name_multiset`] rather than each sorting its own
//! copies. It sits in `relate-model` because that is the crate every one
//! of those four depends on.

/// Multiset equality of two name lists (UNION branch columns).
///
/// UNION branches match by column *name*, not position: the executor
/// reorders each branch's rows onto the head's columns, so the same names in
/// a different order are the same result shape. A multiset comparison rather
/// than a set one: a name projected twice in one branch and once in another
/// is a different shape, not a duplicate to forgive.
///
/// The one spelling of the policy: the parser's subquery-UNION screening, the
/// front's `UNION` validation, the planner's output-name computation and the
/// executor's UNION row alignment all call it, so the four cannot drift.
pub fn same_name_multiset(a: &[String], b: &[String]) -> bool {
    let mut a: Vec<&str> = a.iter().map(String::as_str).collect();
    let mut b: Vec<&str> = b.iter().map(String::as_str).collect();
    a.sort_unstable();
    b.sort_unstable();
    a == b
}

#[cfg(test)]
mod tests {
    use super::same_name_multiset;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn order_is_irrelevant() {
        assert!(same_name_multiset(&names(&["x", "y"]), &names(&["y", "x"])));
    }

    #[test]
    fn duplicates_are_significant() {
        assert!(same_name_multiset(&names(&["x", "x"]), &names(&["x", "x"])));
        assert!(!same_name_multiset(
            &names(&["x", "x", "y"]),
            &names(&["x", "y", "y"])
        ));
    }

    #[test]
    fn different_names_or_lengths_differ() {
        assert!(!same_name_multiset(&names(&["x"]), &names(&["y"])));
        assert!(!same_name_multiset(&names(&["x", "y"]), &names(&["x"])));
        assert!(same_name_multiset(&names(&[]), &names(&[])));
    }
}
