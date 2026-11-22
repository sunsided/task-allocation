//! Unit tests for timed routes and reward shapes.

use alloc::vec;
use alloc::vec::Vec;

use super::rewards::{deadline, halving, linear_decay};
use super::{Route, Timed, Visit};
use crate::id::TaskId;
use crate::valuation::{BestInsertion, PathScore, check_diminishing};

/// A task on a line: where it is, how long it takes, when it may start, and its deadline.
#[derive(Clone, Copy)]
struct Site {
    /// Position on the line.
    at: u64,
    /// Time the task takes.
    duration: u64,
    /// Earliest start.
    opens: u64,
    /// Latest finish.
    closes: u64,
}

/// An agent on a line at unit speed, with walls it cannot cross.
struct Line {
    /// Where the agent starts.
    origin: u64,
    /// The task sites, indexed by task.
    sites: Vec<Site>,
    /// Positions the agent cannot pass.
    walls: Vec<u64>,
}

impl Route for Line {
    type Place = u64;

    fn origin(&self) -> u64 {
        self.origin
    }

    fn departure(&self) -> u64 {
        1
    }

    fn place(&self, task: TaskId) -> u64 {
        self.sites[task.index()].at
    }

    fn travel(&self, from: &u64, to: &u64) -> Option<u64> {
        let (low, high) = if from < to {
            (*from, *to)
        } else {
            (*to, *from)
        };
        let blocked = self.walls.iter().any(|wall| (low..=high).contains(wall));

        (!blocked).then(|| from.abs_diff(*to))
    }

    fn duration(&self, task: TaskId) -> u64 {
        self.sites[task.index()].duration
    }

    fn earliest_start(&self, task: TaskId) -> u64 {
        self.sites[task.index()].opens
    }

    fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
        deadline(10, finish, self.sites[task.index()].closes)
    }
}

/// Three sites: one that opens late, one far away, one behind a wall.
fn line() -> Line {
    Line {
        origin: 0,
        sites: vec![
            Site {
                at: 5,
                duration: 2,
                opens: 20,
                closes: 100,
            },
            Site {
                at: 30,
                duration: 1,
                opens: 0,
                closes: 50,
            },
            Site {
                at: 60,
                duration: 1,
                opens: 0,
                closes: 1000,
            },
        ],
        walls: vec![50],
    }
}

/// An agent that arrives early waits for the task to open.
#[test]
fn early_arrivals_wait() {
    let visits = Timed(line()).schedule(&[TaskId(0), TaskId(1)]).unwrap();

    assert_eq!(
        visits,
        [
            Visit {
                task: TaskId(0),
                arrival: 6,
                start: 20,
                finish: 22,
                reward: 10
            },
            Visit {
                task: TaskId(1),
                arrival: 47,
                start: 47,
                finish: 48,
                reward: 10
            },
        ]
    );
}

/// A late finish or an unreachable place makes the path infeasible.
#[test]
fn late_or_unreachable_paths_are_infeasible() {
    let mut tight = line();
    tight.sites[1].closes = 40;
    let route = Timed(tight);

    assert_eq!(route.path_score(&[TaskId(0), TaskId(1)]), None);
    assert_eq!(route.path_score(&[TaskId(1), TaskId(0)]), Some(20));
    assert_eq!(route.path_score(&[TaskId(2)]), None);
    assert_eq!(route.path_score(&[]), Some(0));
}

/// Fixed rewards with deadlines on a line diminish.
#[test]
fn deadline_routes_diminish() {
    let tasks: Vec<TaskId> = (0..3).map(TaskId).collect();

    assert!(check_diminishing(&BestInsertion(Timed(line())), &tasks, 3).is_ok());
}

/// No reward shape grows with later finish times.
#[test]
fn rewards_never_grow() {
    let mut previous = (Some(u64::MAX), Some(u64::MAX), u64::MAX);
    for finish in 0..300 {
        let current = (
            deadline(77, finish, 150),
            linear_decay(77, finish, 20, 250),
            halving(77, finish, 30),
        );

        assert!(current.0 <= previous.0 && current.1 <= previous.1 && current.2 <= previous.2);
        previous = current;
    }
}

/// Degenerate parameters fall back to a step.
#[test]
fn degenerate_shapes() {
    assert_eq!(linear_decay(9, 5, 5, 5), Some(9));
    assert_eq!(linear_decay(9, 6, 5, 5), None);
    assert_eq!(halving(9, 0, 0), 9);
    assert_eq!(halving(9, 1, 0), 0);
    assert_eq!(halving(u64::MAX, 64 * 5, 5), 0);
}

/// A route that sets only what it must starts at zero, works instantly and never waits.
#[test]
fn defaults_start_at_zero_without_waiting() {
    /// Every task is one step away and worth its index.
    struct Minimal;

    impl Route for Minimal {
        type Place = ();

        fn origin(&self) {}

        fn place(&self, _task: TaskId) {}

        fn travel(&self, _from: &(), _to: &()) -> Option<u64> {
            Some(1)
        }

        fn reward(&self, task: TaskId, _finish: u64) -> Option<u64> {
            u64::try_from(task.index()).ok()
        }
    }

    let visits = Timed(Minimal).schedule(&[TaskId(3), TaskId(4)]).unwrap();

    assert_eq!(
        (visits[0].arrival, visits[0].start, visits[0].finish),
        (1, 1, 1)
    );
    assert_eq!((visits[1].finish, visits[1].reward), (2, 4));
}
