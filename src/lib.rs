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
