//! Unit tests for [`Valuation`], [`BestInsertion`] and [`check_diminishing`].

use super::{BestInsertion, Insertion, PathScore, Valuation, check_diminishing};

/// A task that takes `duration` minutes and must be done `deadline` minutes after the start.
#[derive(Clone)]
struct Task {
    /// Minutes the task takes once started.
    duration: u64,
    /// Minutes after the start by which the task must be done.
    deadline: u64,
    /// Score for doing the task in time.
    reward: u64,
}

/// A rover that works through its bundle in order, one task after another.
struct Rover;

impl Valuation<Task> for Rover {
    fn marginal_score(&self, bundle: &[Task], task: &Task) -> Option<u64> {
        let busy_until: u64 = bundle.iter().map(|held| held.duration).sum();

        (busy_until + task.duration <= task.deadline).then_some(task.reward)
    }
}

/// Visits points on a line, starting at 0. Worth 50 per point plus 100 minus the distance
/// traveled, and infeasible beyond 100.
struct Walker;

impl PathScore<i64> for Walker {
    fn path_score(&self, path: &[i64]) -> Option<u64> {
        let mut at: i64 = 0;
        let mut traveled = 0;

        for &point in path {
            traveled += at.abs_diff(point);
            at = point;
        }

        let visits = 50 * u64::try_from(path.len()).ok()?;

        (100 + visits)
            .checked_sub(traveled)
            .filter(|_| traveled <= 100)
    }
}

/// Picks the index of the task with the highest score, the way a bidding agent does;
/// `None` when the agent can take none of them.
fn best_bid<T, V: Valuation<T>>(agent: &V, bundle: &[T], tasks: &[T]) -> Option<usize> {
    tasks
        .iter()
        .enumerate()
        .filter_map(|(index, task)| Some((agent.marginal_score(bundle, task)?, index)))
        .max()
        .map(|(_, index)| index)
}

/// The agent bids on the most valuable task it can still finish in time.
#[test]
fn bids_on_the_best_feasible_task() {
    let bundle = [Task {
        duration: 30,
        deadline: 60,
        reward: 5,
    }];
    let tasks = [
        Task {
            duration: 20,
            deadline: 40,
            reward: 9,
        },
        Task {
            duration: 10,
            deadline: 50,
            reward: 3,
        },
        Task {
            duration: 25,
            deadline: 90,
            reward: 6,
        },
    ];

    assert_eq!(best_bid(&Rover, &bundle, &tasks), Some(2));
}

/// An agent that can take none of the tasks places no bid.
#[test]
fn no_bid_when_no_task_fits() {
    let bundle = [Task {
        duration: 50,
        deadline: 60,
        reward: 5,
    }];
    let tasks = [Task {
        duration: 20,
        deadline: 40,
        reward: 9,
    }];

    assert_eq!(best_bid(&Rover, &bundle, &tasks), None);
}

/// The default insertion appends the task and reports its marginal score.
#[test]
fn default_insertion_appends() {
    let bundle = [Task {
        duration: 10,
        deadline: 60,
        reward: 5,
    }];
    let task = Task {
        duration: 10,
        deadline: 60,
        reward: 7,
    };

    assert_eq!(
        Rover.best_insertion(&bundle, &task),
        Some(Insertion {
            position: 1,
            gain: 7
        })
    );
}

/// The best insertion of a point is where the detour is shortest.
#[test]
fn best_insertion_minimizes_the_detour() {
    let walker = BestInsertion(Walker);

    // 0 -> 10 -> 20 travels 20. Visiting -5 first adds 10, in the middle 30, last 25.
    assert_eq!(
        walker.best_insertion(&[10, 20], &-5),
        Some(Insertion {
            position: 0,
            gain: 40
        })
    );
    assert_eq!(walker.marginal_score(&[10], &20), Some(40));
}

/// Of equally good positions, the earliest wins.
#[test]
fn ties_go_to_the_earliest_position() {
    let walker = BestInsertion(Walker);

    // A second visit to 10 costs no travel before or after the first one.
    assert_eq!(
        walker.best_insertion(&[10], &10),
        Some(Insertion {
            position: 0,
            gain: 50
        })
    );
}

/// A task that would make the path infeasible has no insertion.
#[test]
fn infeasible_everywhere_gives_none() {
    let walker = BestInsertion(Walker);

    assert_eq!(walker.best_insertion(&[40], &-40), None);
    assert_eq!(walker.best_insertion(&[200], &1), None);
}

/// The deadline rover has diminishing gains: more work only ever makes tasks infeasible.
#[test]
fn deadline_rover_is_diminishing() {
    let tasks = [
        Task {
            duration: 20,
            deadline: 40,
            reward: 9,
        },
        Task {
            duration: 10,
            deadline: 50,
            reward: 3,
        },
        Task {
            duration: 25,
            deadline: 90,
            reward: 6,
        },
    ];

    assert!(check_diminishing(&Rover, &tasks, 3).is_ok());
}

/// A task that only becomes feasible after another one is a violation.
#[test]
fn newly_feasible_task_is_a_violation() {
    /// Can take task 1 only after task 0.
    struct Prerequisite;

    impl Valuation<u8> for Prerequisite {
        fn marginal_score(&self, path: &[u8], task: &u8) -> Option<u64> {
            (*task == 0 || path.contains(&0)).then_some(1)
        }
    }

    let violation = check_diminishing(&Prerequisite, &[0, 1], 2).unwrap_err();

    assert_eq!(violation.added, 0);
    assert_eq!(violation.task, 1);
    assert_eq!((violation.before, violation.after), (None, Some(1)));
}

/// Paths are only explored up to the capacity.
#[test]
fn capacity_bounds_the_search() {
    /// Gains grow only once the path holds two tasks.
    struct LateBloomer;

    impl Valuation<u8> for LateBloomer {
        fn marginal_score(&self, path: &[u8], _task: &u8) -> Option<u64> {
            Some(if path.len() >= 2 { 9 } else { 1 })
        }
    }

    assert!(check_diminishing(&LateBloomer, &[0, 1, 2, 3], 2).is_ok());
    assert!(check_diminishing(&LateBloomer, &[0, 1, 2, 3], 3).is_err());
}
