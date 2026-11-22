//! # Task allocation
//!
//! Decentralized multi-agent task allocation: agents decide among themselves who does
//! which task, exchanging bids whenever they can communicate, without a central authority.
//!
//! ## Scope
//!
//! - Consensus-based bundle algorithm (CBBA) and its variants, converging to conflict-free
//!   assignments under delayed and intermittent communication
//! - Sequential and combinatorial auctions as baselines
//! - Task scores from user-supplied costs, such as travel time, deadlines or resource use
//!
//! Repository: <https://github.com/sunsided/task-allocation>

#![no_std]
#![forbid(unsafe_code)]

/// An agent's valuation of tasks, used to place bids.
pub trait Valuation<Task> {
    /// The score of adding `task` to the agent's current bundle; higher is better, and
    /// `None` means the agent cannot take the task.
    fn marginal_score(&self, bundle: &[Task], task: &Task) -> Option<u64>;
}

/// Doctests for the code examples in README.md (not part of the API).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_doctests {}

/// Unit tests for the `Valuation` trait, through a minimal implementation.
#[cfg(test)]
mod tests {
    use super::Valuation;

    /// A task that takes `duration` minutes and must be done `deadline` minutes after the start.
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
}
