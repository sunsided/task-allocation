//! # Task allocation
//!
//! Decentralized multi-agent task allocation: agents decide among themselves who does
//! which task, exchanging bids whenever they can communicate, without a central authority.
//!
//! ## Building blocks
//!
//! - [`Valuation`]: what a task is worth to one agent, given the tasks it already holds.
//!   Implement it directly, or score whole paths with [`PathScore`] and wrap them in
//!   [`BestInsertion`]. [`check_diminishing`] tests that gains diminish, which the
//!   consensus algorithms' guarantees rely on.
//! - [`Bundle`]: the tasks one agent holds, in the order it took them on and in execution
//!   order, built greedily from marginal gains.
//! - [`Assignment`]: one bundle per agent, the result of every allocation algorithm.
//! - [`route`]: a ready-made path score for agents that travel between task sites, with
//!   service windows and rewards that depend on the finish time.
//! - [`cbba`]: the consensus-based bundle algorithm. Each [`cbba::Agent`] builds its bundle,
//!   broadcasts its view of every task's winner, and merges what it hears, until the team
//!   agrees on a conflict-free assignment. [Bid warping](cbba::warp) extends it to gains
//!   that do not diminish.
//! - [`ccbba`]: CBBA for coupled tasks that need or exclude each other, optionally with
//!   timing limits between their starts on timed routes.
//! - [`acbba`]: asynchronous CBBA, experimental. Agents bid and resolve conflicts on their
//!   own schedule, one task per message, and only pass on what is new; it does not settle
//!   reliably yet.
//! - [`bwtc`]: the task consensus of bid-warped CBBA, experimental. Agents announce what
//!   changed, withdrawals included, with a timestamp per bidder and task, so a withdrawn
//!   bid cannot come back; for networks that deliver every message.
//! - [`sim`]: a deterministic simulated network with delays, losses and partitions, to run
//!   teams on.
//! - [`auction`]: auction baselines with a central auctioneer: sequential single-item
//!   auctions, and combinatorial auctions with exact winner determination.
//! - [`centralized`]: reference allocations with full knowledge of every valuation: the
//!   sequential greedy assignment and the exhaustive optimum for small instances.
//!
//! Agents and tasks are dense indices, [`AgentId`] and [`TaskId`]. Scores are `u64`, so
//! every agent compares bids exactly the same way; ties go to the smaller identifier.
//!
//! The crate is `no_std` and needs `alloc`.
//!
//! ## Example
//!
//! ```
//! use task_allocation::{AgentId, TaskId, Valuation, centralized::sequential_greedy};
//!
//! /// A rover on a line: a task at `position` is worth 100 minus the distance from the
//! /// rover's last task, and each rover takes at most two tasks.
//! struct Rover {
//!     start: u64,
//! }
//!
//! const TASKS: [u64; 3] = [10, 40, 90];
//!
//! impl Valuation<TaskId> for Rover {
//!     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
//!         let from = path.last().map_or(self.start, |last| TASKS[last.index()]);
//!
//!         100_u64.checked_sub(from.abs_diff(TASKS[task.index()]))
//!     }
//! }
//!
//! let rovers = [Rover { start: 0 }, Rover { start: 100 }];
//! let assignment = sequential_greedy(&rovers, TASKS.len(), 2);
//!
//! assert!(assignment.is_conflict_free());
//! assert_eq!(assignment.holder(TaskId(0)), Some(AgentId(0)));
//! assert_eq!(assignment.holder(TaskId(2)), Some(AgentId(1)));
//! ```
//!
//! Repository: <https://github.com/sunsided/task-allocation>

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod acbba;
mod assignment;
pub mod auction;
mod bundle;
pub mod bwtc;
pub mod cbba;
pub mod ccbba;
pub mod centralized;
mod id;
mod quiet;
pub mod route;
pub mod sim;
mod valuation;

pub use assignment::Assignment;
pub use bundle::Bundle;
pub use id::{AgentId, TaskId};
pub use valuation::{BestInsertion, Insertion, PathScore, Valuation, Violation, check_diminishing};

/// Doctests for the code examples in README.md (not part of the API).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_doctests {}
