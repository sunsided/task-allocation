//! The 2010 table, [`Rules::Gnc2010`](super::Rules::Gnc2010): Table 1 of Johnson, Ponda,
//! Choi and How, AIAA GNC 2010, as printed, with its 34 rows.
//!
//! Where it differs from the 2011 table:
//!
//! - Rows 4 and 22 take a newer time for the agreed winner without rebroadcasting it, and
//!   row 24 does not answer an older one.
//! - Rows 10, 11, 28 and 29, where a higher bid is older or a lower bid newer, reset and
//!   pass the sender's claim on, instead of taking the newer claim: the receiver cannot
//!   tell which is right, so it forwards the question to agents that may know.
//! - Rows 20 and 21 take the sender's claim over the sender's own only when it is not
//!   older, and reset otherwise.
//! - Row 14 resets and passes the sender's claim on, and row 16 answers with the
//!   receiver's own entry, where 2011 broadcasts an empty bid at the current time.
//!
//! Rows 1 to 3, 13, 15, 17 to 19 and 31 to 34 are the same in both.

use super::{
    Claim, DEFAULT, Decision, Exchange, leave, quiet, receiver_contested, reset, update,
    update_quietly,
};
use crate::id::AgentId;

impl Exchange {
    /// The decision under the 2010 table.
    pub(super) fn decide_gnc2010(&self, sent: Claim, held: Claim) -> Decision {
        match sent.winner {
            Some(winner) if winner == self.sender => self.gnc_sender_claims_itself(sent, held),
            Some(winner) if winner == self.receiver => self.gnc_sender_claims_receiver(sent, held),
            Some(winner) => self.gnc_sender_claims_other(winner, sent, held),
            // Rows 31 to 34 are the same in both tables.
            None => self.sender_claims_nobody(sent, held),
        }
    }

    /// Rows 1 to 12: the sender believes it wins the task itself.
    fn gnc_sender_claims_itself(&self, sent: Claim, held: Claim) -> Decision {
        match held.winner {
            Some(winner) if winner == self.receiver => receiver_contested(sent, held, [1, 2, 3]),
            Some(winner) if winner == self.sender => self.agreed(sent, held, [4, 5, 6]),
            Some(_) => self.two_winners(sent, held, [7, 8, 9, 10, 11]),
            None => update(12),
        }
    }

    /// Rows 13 to 16: the sender believes the receiver wins the task.
    fn gnc_sender_claims_receiver(&self, sent: Claim, held: Claim) -> Decision {
        match held.winner {
            Some(winner) if winner == self.receiver => {
                if self.same_time(sent, held) {
                    quiet(13)
                } else {
                    DEFAULT
                }
            }
            Some(winner) if winner == self.sender => reset(14),
            Some(_) => leave(15),
            None => leave(16),
        }
    }

    /// Rows 17 to 30: the sender believes a third agent `winner` wins the task.
    fn gnc_sender_claims_other(&self, winner: AgentId, sent: Claim, held: Claim) -> Decision {
        match held.winner {
            Some(held_winner) if held_winner == self.receiver => {
                receiver_contested(sent, held, [17, 18, 19])
            }
            Some(held_winner) if held_winner == self.sender => {
                if sent.time >= held.time {
                    update(20)
                } else {
                    reset(21)
                }
            }
            Some(held_winner) if held_winner == winner => self.agreed(sent, held, [22, 23, 24]),
            Some(_) => self.two_winners(sent, held, [25, 26, 27, 28, 29]),
            None => update(30),
        }
    }

    /// Rows 4 to 6 and 22 to 24: both sides name the same winner. Newer information is
    /// taken, and nothing is rebroadcast either way. `rows` are the rows for newer, equal
    /// and older.
    fn agreed(&self, sent: Claim, held: Claim, rows: [u8; 3]) -> Decision {
        let [newer, equal, older] = rows;

        if self.same_time(sent, held) {
            quiet(equal)
        } else if sent.time > held.time {
            update_quietly(newer)
        } else {
            quiet(older)
        }
    }

    /// Rows 7 to 11 and 25 to 29: sender and receiver name two different winners, neither
    /// of them the receiver. A higher bid that is not older wins, a lower one that is not
    /// newer loses, and when bid and time disagree, the receiver resets and passes the
    /// sender's claim on. `rows` are the rows for a higher and not older bid, a lower and
    /// not newer bid, an equal bid, a lower but newer bid, and a higher but older bid.
    fn two_winners(&self, sent: Claim, held: Claim, rows: [u8; 5]) -> Decision {
        let [higher_newer, lower_older, equal, lower_newer, higher_older] = rows;

        if sent.bid > held.bid && sent.time >= held.time {
            update(higher_newer)
        } else if sent.bid < held.bid && sent.time <= held.time {
            leave(lower_older)
        } else if sent.bid == held.bid {
            self.tie(sent, held, equal)
        } else if sent.bid < held.bid {
            reset(lower_newer)
        } else {
            reset(higher_older)
        }
    }
}

/// Unit tests, one per row of the 2010 table.
#[cfg(test)]
mod tests;
