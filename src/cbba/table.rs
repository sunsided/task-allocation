//! The CBBA conflict-resolution table: what a receiver does with one task's entry in a
//! message.
//!
//! This is the conflict-resolution table of CBBA, from Choi, Brunet and How,
//! "Consensus-Based Decentralized Auctions for Robust Task Allocation" (IEEE Transactions on
//! Robotics, 2009), as implemented in the open-source CBBA-Python
//! (<https://github.com/zehuilu/CBBA-Python>), against which every entry was checked.
//!
//! For each task, the receiver `i` compares the sender `k`'s belief about the winner with its
//! own, and either takes the sender's entry (*update*), forgets the task's winner (*reset*),
//! or keeps its own (*leave*).
//! When a third agent's bid is involved, freshness decides: the sender's information about
//! agent `m` is fresher when it has seen a later clock value of `m` than the receiver has.
//!
//! Ties between bids go to the smaller agent identifier, so "outbids" below means a higher
//! bid, or an equal bid by a smaller agent.

use crate::id::AgentId;

/// One task's entry in an agent's winner tables: who it believes wins, and with what bid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// The believed winner; `None` when the agent knows of no bid on the task.
    pub winner: Option<AgentId>,

    /// The winning bid; zero when there is no winner.
    pub bid: u64,
}

impl Claim {
    /// No known winner.
    pub const NONE: Self = Self {
        winner: None,
        bid: 0,
    };

    /// Whether this claim beats `other`: a higher bid, or an equal bid by a smaller agent.
    /// A claim without a winner beats nothing.
    #[must_use]
    pub fn outbids(self, other: Self) -> bool {
        match (self.winner, other.winner) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(mine), Some(theirs)) => {
                self.bid > other.bid || (self.bid == other.bid && mine < theirs)
            }
        }
    }
}

/// What the receiver does with one task's entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Take the sender's claim.
    Update,

    /// Forget the winner: no known bid on the task.
    Reset,

    /// Keep the receiver's own claim.
    Leave,
}

/// The two sides of one exchange, and their freshness tables.
#[derive(Clone, Copy, Debug)]
pub struct Exchange<'a> {
    /// The agent receiving the message.
    pub receiver: AgentId,

    /// The agent that sent it.
    pub sender: AgentId,

    /// For each agent, the latest clock value of it the sender had seen when it sent.
    pub sender_seen: &'a [u64],

    /// For each agent, the latest clock value of it the receiver has seen.
    pub receiver_seen: &'a [u64],
}

impl Exchange<'_> {
    /// Whether the sender's information about `agent` is strictly fresher than the
    /// receiver's.
    fn sender_fresher(&self, agent: AgentId) -> bool {
        self.sender_seen[agent.index()] > self.receiver_seen[agent.index()]
    }

    /// Whether the receiver's information about `agent` is strictly fresher than the
    /// sender's.
    fn receiver_fresher(&self, agent: AgentId) -> bool {
        self.receiver_seen[agent.index()] > self.sender_seen[agent.index()]
    }

    /// The action for one task, given the sender's claim `sent` and the receiver's claim
    /// `held`.
    #[must_use]
    pub fn resolve(&self, sent: Claim, held: Claim) -> Action {
        match sent.winner {
            Some(winner) if winner == self.sender => self.sender_claims_itself(sent, held),
            Some(winner) if winner == self.receiver => self.sender_claims_receiver(held),
            Some(winner) => self.sender_claims_other(winner, sent, held),
            None => self.sender_claims_nobody(held),
        }
    }

    /// Rows where the sender believes it wins the task itself.
    fn sender_claims_itself(&self, sent: Claim, held: Claim) -> Action {
        match held.winner {
            Some(winner) if winner == self.receiver => update_if(sent.outbids(held)),
            Some(winner) if winner == self.sender => Action::Update,
            Some(other) => update_if(self.sender_fresher(other) || sent.outbids(held)),
            None => Action::Update,
        }
    }

    /// Rows where the sender believes the receiver wins the task.
    fn sender_claims_receiver(&self, held: Claim) -> Action {
        match held.winner {
            Some(winner) if winner == self.receiver => Action::Leave,
            Some(winner) if winner == self.sender => Action::Reset,
            Some(other) if self.sender_fresher(other) => Action::Reset,
            Some(_) | None => Action::Leave,
        }
    }

    /// Rows where the sender believes a third agent `winner` wins the task.
    fn sender_claims_other(&self, winner: AgentId, sent: Claim, held: Claim) -> Action {
        let fresher = self.sender_fresher(winner);

        match held.winner {
            Some(held_winner) if held_winner == self.receiver => {
                update_if(fresher && sent.outbids(held))
            }
            Some(held_winner) if held_winner == self.sender => {
                if fresher {
                    Action::Update
                } else {
                    Action::Reset
                }
            }
            Some(held_winner) if held_winner == winner => update_if(fresher),
            Some(held_winner) => self.two_third_parties(winner, held_winner, sent, held),
            None => update_if(fresher),
        }
    }

    /// The row where sender and receiver believe two different third agents win: the
    /// sender's `winner` and the receiver's `held_winner`.
    ///
    /// When the sender knows `held_winner` better, it updates unless the receiver knows
    /// `winner` better, which resets. When both know `winner` equally well, this updates,
    /// as the open-source CBBA-Python implementation of the table does
    /// (<https://github.com/zehuilu/CBBA-Python>).
    fn two_third_parties(
        &self,
        winner: AgentId,
        held_winner: AgentId,
        sent: Claim,
        held: Claim,
    ) -> Action {
        if self.sender_fresher(held_winner) {
            if self.receiver_fresher(winner) {
                Action::Reset
            } else {
                Action::Update
            }
        } else {
            update_if(self.sender_fresher(winner) && sent.outbids(held))
        }
    }

    /// Rows where the sender knows of no winner.
    fn sender_claims_nobody(&self, held: Claim) -> Action {
        match held.winner {
            Some(winner) if winner == self.receiver => Action::Leave,
            Some(winner) if winner == self.sender => Action::Update,
            Some(other) => update_if(self.sender_fresher(other)),
            None => Action::Leave,
        }
    }
}

/// [`Action::Update`] when `condition` holds, [`Action::Leave`] otherwise.
const fn update_if(condition: bool) -> Action {
    if condition {
        Action::Update
    } else {
        Action::Leave
    }
}

/// Unit tests for the conflict-resolution table, row by row.
#[cfg(test)]
mod tests;
