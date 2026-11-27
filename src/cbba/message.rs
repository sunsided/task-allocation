//! What CBBA agents exchange.

use alloc::vec::Vec;

use super::table::Claim;
use crate::id::{AgentId, TaskId};

/// One agent's broadcast: its belief about every task's winner and winning bid, and the
/// latest clock value it has seen of every agent.
///
/// A message is a snapshot of the sender's state, so a receiver can merge it whenever it
/// arrives, in any order, any number of times.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// The agent that sent the message.
    pub(super) sender: AgentId,

    /// The sender's winner and winning bid for each task, indexed by task.
    pub(super) claims: Vec<Claim>,

    /// For each agent, the latest clock value of it the sender had seen; the sender's own
    /// entry is its clock when it sent the message.
    pub(super) seen: Vec<u64>,
}

impl Message {
    /// The agent that sent the message.
    #[must_use]
    pub const fn sender(&self) -> AgentId {
        self.sender
    }

    /// The sender's clock when it sent the message: how many messages it had sent,
    /// this one included.
    #[must_use]
    pub fn clock(&self) -> u64 {
        self.seen[self.sender.index()]
    }

    /// The winner the sender believed in for `task`; `None` when it knew of no bid.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of the message's tasks.
    #[must_use]
    pub fn winner(&self, task: TaskId) -> Option<AgentId> {
        self.claims[task.index()].winner
    }

    /// The winning bid the sender believed in for `task`; zero when it knew of no bid.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of the message's tasks.
    #[must_use]
    pub fn winning_bid(&self, task: TaskId) -> u64 {
        self.claims[task.index()].bid
    }

    /// The sender's winner and winning bid for each task, in task order.
    pub(crate) fn claims(&self) -> &[Claim] {
        &self.claims
    }

    /// The number of tasks the message covers.
    #[must_use]
    pub fn tasks(&self) -> usize {
        self.claims.len()
    }

    /// The message's nominal size on the wire, in bytes: the sender, then every task's
    /// winner and bid, then every agent's clock, each list after its length.
    pub(crate) fn bytes(&self) -> u64 {
        let claims = u64::try_from(self.claims.len()).unwrap_or(u64::MAX);
        let seen = u64::try_from(self.seen.len()).unwrap_or(u64::MAX);

        4 + 4 + 12 * claims + 4 + 8 * seen
    }
}
