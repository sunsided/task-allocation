//! Unit tests for [`Bundle`].

use alloc::vec::Vec;

use super::Bundle;
use crate::valuation::{Insertion, Valuation};

/// Values task `t` at `t`, inserting each new task at the front of the path.
struct FrontLoader;

impl Valuation<u64> for FrontLoader {
    fn marginal_score(&self, _path: &[u64], task: &u64) -> Option<u64> {
        Some(*task)
    }

    fn best_insertion(&self, _path: &[u64], task: &u64) -> Option<Insertion> {
        Some(Insertion {
            position: 0,
            gain: *task,
        })
    }
}

/// Bundle order and path order are tracked separately.
#[test]
fn bundle_and_path_orders_differ() {
    let mut bundle = Bundle::new();
    bundle.extend_greedily(&FrontLoader, &[1, 3, 2], 3, |_, _| true);

    assert_eq!(bundle.tasks().copied().collect::<Vec<_>>(), [3, 2, 1]);
    assert_eq!(bundle.path(), [1, 2, 3]);
    assert_eq!(bundle.value(), 6);
}

/// Releasing a task drops it and everything added after it, from both orders.
#[test]
fn release_drops_later_tasks() {
    let mut bundle = Bundle::new();
    bundle.extend_greedily(&FrontLoader, &[1, 3, 2, 4], 4, |_, _| true);

    let released = bundle.release_from(bundle.index_of(&3).unwrap());

    assert_eq!(released, [3, 2, 1]);
    assert_eq!(bundle.path(), [4]);
    assert_eq!(bundle.entries().collect::<Vec<_>>(), [(&4, 4)]);
    assert_eq!(bundle.release_from(5), []);
}

/// Rejected tasks are skipped, and the next best one is taken.
#[test]
fn accept_filters_candidates() {
    let mut bundle = Bundle::new();
    let added = bundle.extend_greedily(&FrontLoader, &[1, 3, 2], 3, |task, _| *task != 3);

    assert_eq!(added, 2);
    assert_eq!(bundle.tasks().copied().collect::<Vec<_>>(), [2, 1]);
}

/// Of equal gains, the candidate listed first wins.
#[test]
fn ties_go_to_the_first_candidate() {
    /// Values every task the same.
    struct Flat;

    impl Valuation<u64> for Flat {
        fn marginal_score(&self, _path: &[u64], _task: &u64) -> Option<u64> {
            Some(1)
        }
    }

    let mut bundle = Bundle::new();
    bundle.extend_greedily(&Flat, &[7, 5, 6], 1, |_, _| true);

    assert_eq!(bundle.path(), [7]);
}

/// Construction stops at the capacity, and when nothing is feasible.
#[test]
fn stops_at_capacity_or_when_nothing_fits() {
    /// Can take at most two tasks.
    struct Small;

    impl Valuation<u64> for Small {
        fn marginal_score(&self, path: &[u64], task: &u64) -> Option<u64> {
            (path.len() < 2).then_some(*task)
        }
    }

    let tasks = [1, 2, 3, 4];
    let mut capped = Bundle::new();
    let mut unbounded = Bundle::new();

    assert_eq!(capped.extend_greedily(&Small, &tasks, 1, |_, _| true), 1);
    assert_eq!(
        unbounded.extend_greedily(&Small, &tasks, 10, |_, _| true),
        2
    );
    assert_eq!(unbounded.path(), [4, 3]);
}

/// Inserting past the end of the path is a caller bug.
#[test]
#[should_panic(expected = "beyond the path's length")]
fn insert_past_the_end_panics() {
    let mut bundle = Bundle::new();
    bundle.insert(
        1,
        Insertion {
            position: 1,
            gain: 1,
        },
    );
}

/// Clearing empties both orders.
#[test]
fn clear_empties_the_bundle() {
    let mut bundle = Bundle::default();
    bundle.extend_greedily(&FrontLoader, &[1, 2], 2, |_, _| true);
    bundle.clear();

    assert!(bundle.is_empty());
    assert_eq!(bundle.path(), []);
}
