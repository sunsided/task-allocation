//! Bid warping: CBBA for valuations whose marginal gains do not diminish.
//!
//! CBBA's consensus assumes that an agent's bids never grow along its bundle. A valuation
//! that rewards tasks for fitting together breaks that: a courier that already drives to
//! one site may value a nearby site more than it valued the first one. Two agents can then
//! outbid each other's first tasks with their second bids, both release everything, and the
//! team repeats the same round forever (Example 2 of the paper below).
//!
//! With warping, an agent still ranks its candidate tasks by their true marginal gain, but
//! bids at most what it bid on the task it added last:
//!
//! ```text
//! bid = min(gain, last bid in the bundle)
//! ```
//!
//! Shared bids then never grow along a bundle, so consensus behaves as if gains diminished.
//! An agent can win a task only when its warped bid beats the best known bid, and its
//! bundle records the warped bids: [`Bundle::value`](crate::Bundle::value) sums the bids,
//! not the true value of the path.
//!
//! The method is bid-warped CBBA (BW-CBBA) from Johnson, Choi, Ponda and How,
//! "Decentralized Task Allocation Using Local Information Consistency Assumptions", Journal
//! of Aerospace Information Systems 14(2), 2017 (<https://doi.org/10.2514/1.I010461>),
//! Sec. IV.B, Eqs. 11–12 and Algorithm 3. The paper proves convergence for its own task
//! consensus ([`bwtc`](crate::bwtc)) in synchronous, reliable rounds. Here, warping sits on
//! top of CBBA's conflict table and full-state messages; that it settles over delayed, lost
//! and reordered messages is shown by property tests, not proven. CBBA's guarantee of half
//! the optimal value assumes diminishing gains, and does not carry over.

use super::Agent;
use crate::bundle::Bundle;
use crate::id::TaskId;
use crate::valuation::Valuation;

impl<V: Valuation<TaskId>> Agent<V> {
    /// This agent, warping its bids so that they never grow along its bundle: for
    /// valuations whose marginal gains do not diminish.
    ///
    /// The agent ranks tasks by their true marginal gain, but bids at most what it bid on
    /// the task it added last; see the [module docs](self) for the method and its limits.
    /// With diminishing gains, warping changes nothing.
    #[must_use]
    pub const fn with_bid_warping(mut self) -> Self {
        self.warping = true;
        self
    }
}

/// The most an agent may bid on its next task: with `warping`, the bid on the task it
/// added last to `bundle`; otherwise, or for an empty bundle, no limit.
pub(super) fn ceiling(bundle: &Bundle<TaskId>, warping: bool) -> u64 {
    bundle
        .entries()
        .last()
        .filter(|_| warping)
        .map_or(u64::MAX, |(_, bid)| bid)
}

/// Unit tests for bid warping.
#[cfg(test)]
mod tests;
