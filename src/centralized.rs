//! Centralized reference allocations, computed with full knowledge of every agent's
//! valuation.
//!
//! These are not decentralized: one process sees every valuation. They serve as references
//! for the decentralized algorithms, and as fallbacks when a central planner is available.
//!
//! - [`sequential_greedy`] repeatedly gives the single most valuable remaining task to the
//!   agent that values it most. When the valuations are submodular set functions (the order
//!   of tasks does not matter and gains diminish), it scores at least half the optimum if
//!   agents have no capacity limit (Lehmann, Lehmann and Nisan, 2001), and at least a third
//!   with one (Fisher, Nemhauser and Wolsey, 1978: greedy over two matroids).
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
