//! The ascending id-list algebra (LABS-803): union and intersection as
//! two-pointer merges over the ascending, deduplicated id lists every
//! serve and lookup produces — iterating the lists directly instead of
//! re-materializing them into a per-statement `HashSet<NodeId>`.
//!
//! Home: the lowest module both consumers import — this overlay's own
//! scratch merge (`seek.rs`) and the pattern-walk tier's candidate
//! folds (which sit above the overlay and read it downward), so the one
//! spelling serves both without an upward edge.

use std::cmp::Ordering;

use relate_core::NodeId;

/// The union of two ascending, deduplicated id lists — a two-pointer
/// merge whose answer keeps the same ascending, deduplicated contract,
/// so every later fold — and every membership probe, a binary search on
/// the sorted slice — stays correct.
pub(crate) fn union_ids(a: Vec<NodeId>, b: Vec<NodeId>) -> Vec<NodeId> {
    let mut ids = Vec::with_capacity(a.len() + b.len());
    let (mut left, mut right) = (0, 0);
    while left < a.len() && right < b.len() {
        match a[left].cmp(&b[right]) {
            Ordering::Less => {
                ids.push(a[left]);
                left += 1;
            }
            Ordering::Greater => {
                ids.push(b[right]);
                right += 1;
            }
            Ordering::Equal => {
                ids.push(a[left]);
                left += 1;
                right += 1;
            }
        }
    }
    ids.extend_from_slice(&a[left..]);
    ids.extend_from_slice(&b[right..]);
    ids
}

/// The intersection of two ascending, deduplicated id lists — the
/// two-pointer twin of [`union_ids`], answer ascending and deduplicated.
pub(crate) fn intersect_ids(a: Vec<NodeId>, b: Vec<NodeId>) -> Vec<NodeId> {
    let mut ids = Vec::with_capacity(a.len().min(b.len()));
    let (mut left, mut right) = (0, 0);
    while left < a.len() && right < b.len() {
        match a[left].cmp(&b[right]) {
            Ordering::Less => left += 1,
            Ordering::Greater => right += 1,
            Ordering::Equal => {
                ids.push(a[left]);
                left += 1;
                right += 1;
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Union merges two ascending lists without duplicating the shared
    /// ids — the two-pointer algebra every OR composite and the overlay's
    /// scratch merge read.
    #[test]
    fn union_merges_sorted_lists_without_duplicates() {
        assert_eq!(
            union_ids(vec![1, 3, 5, 7], vec![3, 4, 7, 9]),
            vec![1, 3, 4, 5, 7, 9]
        );
        // One side subsuming the other, and an empty side, keep the answer
        // the subsumed list alone would give.
        assert_eq!(union_ids(vec![2, 4, 6], vec![4, 6]), vec![2, 4, 6]);
        assert_eq!(union_ids(Vec::new(), vec![1, 2]), vec![1, 2]);
        assert_eq!(union_ids(vec![1, 2], Vec::new()), vec![1, 2]);
    }

    /// Intersection keeps the shared ids of two ascending lists, ascending
    /// — the two-pointer algebra every AND composite and the far probe's
    /// fold read.
    #[test]
    fn intersection_keeps_the_shared_ids_ascending() {
        assert_eq!(
            intersect_ids(vec![1, 3, 5, 7], vec![3, 4, 7, 9]),
            vec![3, 7]
        );
        // Disjoint lists intersect to nothing, an empty side to nothing.
        assert_eq!(intersect_ids(vec![1, 2], vec![3, 4]), Vec::<NodeId>::new());
        assert_eq!(intersect_ids(Vec::new(), vec![1, 2]), Vec::<NodeId>::new());
        assert_eq!(intersect_ids(vec![1, 2], Vec::new()), Vec::<NodeId>::new());
    }
}
