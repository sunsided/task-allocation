//! Centralized reference allocations, computed with full knowledge of every agent's
//! valuation.
//!
//! These are not decentralized: one process sees every valuation. They serve as references
//! for the decentralized algorithms, and as fallbacks when a central planner is available.
//!
//! - [`sequential_greedy`] repeatedly gives the single most valuable remaining task to the
//!   agent that values it most. With diminishing marginal gains, CBBA converges to exactly
//!   this assignment, and it scores at least half the optimum.
//! - [`optimal`] finds the best assignment by exhaustive search, for small instances.
//!
//! Both maximize the same objective, [`Assignment::value`]: the sum, over agents, of the
//! marginal gains recorded while each bundle was built.
//!
//! [`Assignment::value`]: crate::Assignment::value

mod exact;
mod greedy;

pub use exact::{MAX_OPTIMAL_TASKS, optimal};
pub use greedy::sequential_greedy;
