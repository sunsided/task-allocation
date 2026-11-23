//! The ACBBA deconfliction tables: what a receiver does with one task's entry in a message,
//! and what it broadcasts in response.
//!
//! Two published revisions of the table differ, and an agent follows one of them, chosen
//! with [`Rules`]:
//!
//! - [`Rules::Gnc2010`]: Table 1 of Johnson, Ponda, Choi and How, "Improving the Efficiency
//!   of a Decentralized Tasking Algorithm for UAV Teams with Asynchronous Communications",
//!   AIAA GNC 2010 (<https://doi.org/10.2514/6.2010-8421>). Where the bids and times of two
//!   different winners point different ways, the receiver forgets its winner and passes the
//!   sender's claim on, and newer information about an agreed winner is taken without
//!   rebroadcasting it. Its rules live in [`gnc2010`].
//! - [`Rules::Infotech2011`]: Table 1 of the same authors' "Asynchronous Decentralized Task
//!   Allocation for Dynamic Environments", AIAA Infotech@Aerospace 2011
//!   (<https://doi.org/10.2514/6.2011-1441>), which cites the 2010 paper for its details.
//!   It takes the newer claim in those cases instead. That table has no rows 21 and 27,
//!   which fall to the default. Its rules live in this module.
//!
//! The rule numbers below are each table's row numbers. Unlike CBBA's table, each claim
//! carries the time its bid was made (`t`), and every rule also decides what to rebroadcast,
//! so agents only pass on what is new.
//!
//! Choices the papers leave open:
//!
//! - Two times count as equal when they differ by less than `epsilon` (`ε_t` in the
//!   papers, which call it small but give no value). When a time is both later and equal
//!   within `epsilon`, as rows 4 and 5 can be, the equality rule wins: the information is
//!   the same.
//! - What time a reset leaves: see [`Rules`].
//!
//! One deliberate deviation, which [`Exchange::third_party_ties`] turns off: with equal
//! bids for two different winners, row 9 and row 27 (the missing row 27 in 2011) leave the
//! receiver's claim, on both sides, so two neighbors that disagree about a tie answer each
//! other forever. The 2010 paper expects the prospective winners to break such ties once
//! they hear of them; with integer scores, ties are common enough that this did not settle
//! in simulation. Here the smaller winner takes the tie, as rows 2 and 18 already do, and
//! the rows report as 9 and 27.

use crate::id::AgentId;

/// One task's entry: who wins it, with what bid, and when that bid was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Claim {
    /// The believed winner; `None` when there is no known bid.
    pub winner: Option<AgentId>,

    /// The winning bid; zero when there is no winner.
    pub bid: u64,

    /// When the bid was made or last confirmed, or when the task was last reset, in ticks of
    /// a clock all agents share.
    pub time: u64,
}

impl Claim {
    /// No known winner, as of time zero.
    pub const NONE: Self = Self {
        winner: None,
        bid: 0,
        time: 0,
    };

    /// No known winner, as of `time`.
    #[must_use]
    pub const fn empty_at(time: u64) -> Self {
        Self {
            winner: None,
            bid: 0,
            time,
        }
    }
}

/// How the receiver changes its own entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Take the sender's claim.
    Update,

    /// Keep its own claim, but stamp it with the current time: it is the winner, and
    /// confirms the bid is still active.
    UpdateTime,

    /// Take the sender's claim, and when it is not newer than the receiver's, stamp it
    /// `epsilon` after the receiver's time; the thesis's "update†", as inferred.
    UpdatePast,

    /// Keep its own claim, and when it is not newer than the sender's, stamp it `epsilon`
    /// after the sender's time; the thesis's "update‡" and "leave‡", as inferred.
    KeepPast,

    /// Forget the winner.
    Reset,

    /// Keep its own claim.
    Leave,
}

/// What the receiver broadcasts in response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Broadcast {
    /// Nothing.
    Nothing,

    /// Its own entry, after the change.
    Own,

    /// The sender's claim, as received.
    Sender,

    /// An empty bid, stamped with the current time.
    EmptyNow,
}

/// The outcome of one table lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    /// How the receiver changes its entry.
    pub change: Change,

    /// What it broadcasts.
    pub broadcast: Broadcast,

    /// The table row that applied; zero for the default, leave and rebroadcast.
    pub row: u8,
}

/// Which published decision table an agent follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Rules {
    /// Table 1 of the 2010 GNC paper, the original ACBBA. A reset forgets the winner and
    /// the time, as an agent that never heard of the task, so that only the sender's claim,
    /// which it passes on, travels further: with time zero, row 33 never spreads the reset
    /// as if it were a released bid, and rows 12 and 30 still take any later claim. The
    /// paper only says the winner and bid are cleared. Stamping the reset with the current
    /// time instead gives the same outcomes in the comparison matrix.
    Gnc2010,

    /// Table 1 of the 2011 Infotech@Aerospace paper. A reset forgets the winner and stamps
    /// the entry with the current time, the same empty bid that rows 14 and 16 broadcast.
    /// The default: of the three, it settles most often in simulation.
    #[default]
    Infotech2011,

    /// Table 4.1 of Johnson's thesis, "Decentralized Task Allocation for Dynamic
    /// Environments" (MIT, 2012): the 2011 table with times pushed past the losing claim's
    /// in rows 1, 3, 13, 15, 17, 19, 31 and 32 (2011 numbering), as [`thesis2012`]
    /// describes. A reset works as in 2011.
    Thesis2012,
}

/// The two sides of one exchange.
#[derive(Clone, Copy, Debug)]
pub struct Exchange {
    /// The agent receiving the message.
    pub receiver: AgentId,

    /// The agent that sent it.
    pub sender: AgentId,

    /// Times closer than this count as equal; `ε_t` in the paper.
    pub epsilon: u64,

    /// The table to follow.
    pub rules: Rules,

    /// Whether a receiver that is neither of two tied winners breaks the tie for the
    /// smaller one, in rows 9 and 27; the published tables leave both claims as they are.
    pub third_party_ties: bool,
}

/// Update and rebroadcast the sender's information.
const fn update(row: u8) -> Decision {
    Decision {
        change: Change::Update,
        broadcast: Broadcast::Sender,
        row,
    }
}

/// Leave, and rebroadcast own information.
const fn leave(row: u8) -> Decision {
    Decision {
        change: Change::Leave,
        broadcast: Broadcast::Own,
        row,
    }
}

/// Update, and broadcast nothing.
const fn update_quietly(row: u8) -> Decision {
    Decision {
        change: Change::Update,
        broadcast: Broadcast::Nothing,
        row,
    }
}

/// Forget the winner, and rebroadcast the sender's information.
const fn reset(row: u8) -> Decision {
    Decision {
        change: Change::Reset,
        broadcast: Broadcast::Sender,
        row,
    }
}

/// Leave, and broadcast nothing.
const fn quiet(row: u8) -> Decision {
    Decision {
        change: Change::Leave,
        broadcast: Broadcast::Nothing,
        row,
    }
}

/// The default: leave and rebroadcast own information.
const DEFAULT: Decision = leave(0);

impl Exchange {
    /// The decision for one task, given the sender's claim `sent` and the receiver's claim
    /// `held`.
    #[must_use]
    pub fn decide(&self, sent: Claim, held: Claim) -> Decision {
        match self.rules {
            Rules::Gnc2010 => self.decide_gnc2010(sent, held),
            Rules::Infotech2011 => self.decide_infotech2011(sent, held),
            Rules::Thesis2012 => thesis2012::decide(self.decide_infotech2011(sent, held)),
        }
    }

    /// The decision under the 2011 table.
    fn decide_infotech2011(&self, sent: Claim, held: Claim) -> Decision {
        match sent.winner {
            Some(winner) if winner == self.sender => self.sender_claims_itself(sent, held),
            Some(winner) if winner == self.receiver => self.sender_claims_receiver(sent, held),
            Some(winner) => self.sender_claims_other(winner, sent, held),
            None => self.sender_claims_nobody(sent, held),
        }
    }

    /// Whether the two times are equal within `epsilon`.
    const fn same_time(&self, sent: Claim, held: Claim) -> bool {
        sent.time.abs_diff(held.time) < self.epsilon
    }

    /// Rows 1 to 12: the sender believes it wins the task itself.
    fn sender_claims_itself(&self, sent: Claim, held: Claim) -> Decision {
        match held.winner {
            Some(winner) if winner == self.receiver => receiver_contested(sent, held, [1, 2, 3]),
            Some(winner) if winner == self.sender => self.newer_wins(sent, held, [4, 5, 6], false),
            Some(_) => self.bids_and_times(sent, held, [7, 8, 9, 10, 11], true),
            None => update(12),
        }
    }

    /// Rows 13 to 16: the sender believes the receiver wins the task.
    fn sender_claims_receiver(&self, sent: Claim, held: Claim) -> Decision {
        match held.winner {
            Some(winner) if winner == self.receiver => {
                if self.same_time(sent, held) {
                    quiet(13)
                } else {
                    DEFAULT
                }
            }
            Some(winner) if winner == self.sender => Decision {
                change: Change::Reset,
                broadcast: Broadcast::EmptyNow,
                row: 14,
            },
            Some(_) => leave(15),
            None => Decision {
                change: Change::Leave,
                broadcast: Broadcast::EmptyNow,
                row: 16,
            },
        }
    }

    /// Rows 17 to 30: the sender believes a third agent `winner` wins the task.
    fn sender_claims_other(&self, winner: AgentId, sent: Claim, held: Claim) -> Decision {
        match held.winner {
            Some(held_winner) if held_winner == self.receiver => {
                receiver_contested(sent, held, [17, 18, 19])
            }
            Some(held_winner) if held_winner == self.sender => update(20),
            Some(held_winner) if held_winner == winner => {
                self.newer_wins(sent, held, [22, 23, 24], true)
            }
            Some(_) => self.bids_and_times(sent, held, [25, 26, 27, 28, 29], false),
            None => update(30),
        }
    }

    /// Rows 31 to 34: the sender knows of no winner.
    fn sender_claims_nobody(&self, sent: Claim, held: Claim) -> Decision {
        match held.winner {
            Some(winner) if winner == self.receiver => leave(31),
            Some(winner) if winner == self.sender => update(32),
            Some(_) if sent.time > held.time => update(33),
            Some(_) => DEFAULT,
            None => quiet(34),
        }
    }

    /// Rows 4 to 6 and 22 to 24: both sides name the same winner, so the newer information
    /// wins. `rows` are the rows for newer, equal and older; with `older_rebroadcasts`, an
    /// older claim is answered with the receiver's own.
    fn newer_wins(
        &self,
        sent: Claim,
        held: Claim,
        rows: [u8; 3],
        older_rebroadcasts: bool,
    ) -> Decision {
        let [newer, equal, older] = rows;

        if self.same_time(sent, held) {
            quiet(equal)
        } else if sent.time > held.time {
            update(newer)
        } else if older_rebroadcasts {
            leave(older)
        } else {
            quiet(older)
        }
    }
}

/// Rows 1 to 3 and 17 to 19: the receiver believes it wins the task, the sender disagrees.
/// A higher bid, or an equal bid by a smaller agent, wins; a lower one makes the receiver
/// confirm its own bid with the current time.
fn receiver_contested(sent: Claim, held: Claim, rows: [u8; 3]) -> Decision {
    let [higher, tie, lower] = rows;

    if sent.bid > held.bid {
        update(higher)
    } else if sent.bid == held.bid && sent.winner < held.winner {
        update(tie)
    } else if sent.bid < held.bid {
        Decision {
            change: Change::UpdateTime,
            broadcast: Broadcast::Own,
            row: lower,
        }
    } else {
        DEFAULT
    }
}

impl Exchange {
    /// Rows 7 to 11 and 25 to 29 of the 2011 table: sender and receiver name two different
    /// winners, neither of them the receiver, so bids and times decide. `rows` are the rows
    /// for a higher and not older bid, a lower and not newer bid, an equal bid, a lower but
    /// newer bid, and a higher but older bid. With `higher_older_updates`, a higher but
    /// older bid updates (row 11), otherwise it is answered (row 29).
    fn bids_and_times(
        &self,
        sent: Claim,
        held: Claim,
        rows: [u8; 5],
        higher_older_updates: bool,
    ) -> Decision {
        let [higher_newer, lower_older, equal, lower_newer, higher_older] = rows;

        if sent.bid > held.bid && sent.time >= held.time {
            update(higher_newer)
        } else if sent.bid < held.bid && sent.time <= held.time {
            leave(lower_older)
        } else if sent.bid == held.bid {
            self.tie(sent, held, equal)
        } else if sent.bid < held.bid {
            update(lower_newer)
        } else if higher_older_updates {
            update(higher_older)
        } else {
            leave(higher_older)
        }
    }

    /// Row 9 or 27, `row`: equal bids for two different winners, neither of them the
    /// receiver. The published tables leave and rebroadcast; with
    /// [`third_party_ties`](Self::third_party_ties), the smaller winner takes the tie.
    fn tie(&self, sent: Claim, held: Claim, row: u8) -> Decision {
        if self.third_party_ties && sent.winner < held.winner {
            update(row)
        } else {
            leave(row)
        }
    }
}

mod gnc2010;
mod thesis2012;

/// Unit tests, one per row of the 2011 table.
#[cfg(test)]
mod tests;
