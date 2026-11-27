//! Counting quiet rounds for local settle detection; see
//! [`cbba::Agent::is_locally_settled`](crate::cbba::Agent::is_locally_settled) for the
//! heuristic and its limits.

use alloc::vec::Vec;

/// How many bundle phases in a row an agent's winners and bids stayed the same.
#[derive(Clone, Debug)]
pub(crate) struct Quiet<T> {
    /// The winners and bids at the last bundle phase that changed them.
    last: Vec<T>,

    /// Bundle phases since then.
    rounds: u64,
}

impl<T> Quiet<T> {
    /// A count that has seen no bundle phase yet.
    pub(crate) const fn new() -> Self {
        Self {
            last: Vec::new(),
            rounds: 0,
        }
    }

    /// Bundle phases since the winners or bids last changed.
    pub(crate) const fn rounds(&self) -> u64 {
        self.rounds
    }
}

impl<T: Clone + PartialEq> Quiet<T> {
    /// Notes one bundle phase, ending with `view`: the agent's winners and bids per task.
    /// Counts it as quiet when `view` matches the last one, and restarts the count
    /// otherwise. Does not allocate once the task count stopped growing.
    pub(crate) fn observe(&mut self, view: impl ExactSizeIterator<Item = T> + Clone) {
        if self.last.len() == view.len() && self.last.iter().cloned().eq(view.clone()) {
            self.rounds = self.rounds.saturating_add(1);
            return;
        }

        self.last.clear();
        self.last.extend(view);
        self.rounds = 0;
    }
}

/// Unit tests for [`Quiet`].
#[cfg(test)]
mod tests;
