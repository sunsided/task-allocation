//! One check per row of the 2010 table, with receiver 1, sender 2 and third agents 0 and 3,
//! read off Table 1 of the GNC 2010 paper.
//!
//! Agent 0 is the smallest identifier, so ties can go either way.

use super::super::{Broadcast, Change, Claim, Decision, Exchange, Rules};
use crate::id::AgentId;

/// The receiver.
const I: AgentId = AgentId(1);
/// The sender.
const K: AgentId = AgentId(2);
/// A third agent, smaller than both.
const M: AgentId = AgentId(0);
/// Another third agent.
const N: AgentId = AgentId(3);

/// Take the sender's claim and pass it on.
const UPDATE: (Change, Broadcast) = (Change::Update, Broadcast::Sender);
/// Take the sender's claim and say nothing.
const UPDATE_QUIETLY: (Change, Broadcast) = (Change::Update, Broadcast::Nothing);
/// Confirm the receiver's own bid and pass it on.
const UPDATE_TIME: (Change, Broadcast) = (Change::UpdateTime, Broadcast::Own);
/// Forget the winner and pass the sender's claim on.
const RESET: (Change, Broadcast) = (Change::Reset, Broadcast::Sender);
/// Keep the receiver's claim and pass it on.
const LEAVE: (Change, Broadcast) = (Change::Leave, Broadcast::Own);
/// Keep the receiver's claim and say nothing.
const QUIET: (Change, Broadcast) = (Change::Leave, Broadcast::Nothing);

/// A claim that `winner` wins with `bid`, made at `time`.
const fn claim(winner: AgentId, bid: u64, time: u64) -> Claim {
    Claim {
        winner: Some(winner),
        bid,
        time,
    }
}

/// No winner, as of `time`.
const fn none(time: u64) -> Claim {
    Claim::empty_at(time)
}

/// The exchange from sender `sender` to receiver `receiver`, with times equal below
/// `epsilon` ticks and ties left as printed.
const fn exchange(receiver: AgentId, sender: AgentId, epsilon: u64) -> Exchange {
    Exchange {
        receiver,
        sender,
        epsilon,
        rules: Rules::Gnc2010,
        third_party_ties: false,
    }
}

/// Checks that `sent` against `held`, from the sender to the receiver, applies `row` with
/// `action`.
fn row(row: u8, sent: Claim, held: Claim, action: (Change, Broadcast)) {
    let (change, broadcast) = action;

    assert_eq!(
        exchange(I, K, 1).decide(sent, held),
        Decision {
            change,
            broadcast,
            row
        },
        "row {row}"
    );
}

/// Rows 1 to 3: the sender claims the task, the receiver believes it wins it.
#[test]
fn rows_1_to_3() {
    row(1, claim(K, 9, 5), claim(I, 5, 5), UPDATE);
    row(3, claim(K, 4, 5), claim(I, 5, 5), UPDATE_TIME);
    row(0, claim(K, 5, 5), claim(I, 5, 5), LEAVE);

    assert_eq!(
        exchange(K, I, 1).decide(claim(I, 5, 5), claim(K, 5, 5)),
        Decision {
            change: Change::Update,
            broadcast: Broadcast::Sender,
            row: 2
        }
    );
}

/// Rows 4 to 6: both believe the sender wins; a newer time is taken silently.
#[test]
fn rows_4_to_6() {
    row(4, claim(K, 5, 9), claim(K, 5, 5), UPDATE_QUIETLY);
    row(5, claim(K, 5, 5), claim(K, 5, 5), QUIET);
    row(6, claim(K, 5, 2), claim(K, 5, 5), QUIET);
}

/// Rows 7 to 11: the sender claims the task, the receiver believes a third agent wins it.
#[test]
fn rows_7_to_11() {
    row(7, claim(K, 9, 5), claim(M, 5, 5), UPDATE);
    row(8, claim(K, 4, 5), claim(M, 5, 5), LEAVE);
    row(9, claim(K, 5, 5), claim(N, 5, 5), LEAVE);
    row(9, claim(K, 5, 5), claim(M, 5, 5), LEAVE);
    row(10, claim(K, 4, 9), claim(M, 5, 5), RESET);
    row(11, claim(K, 9, 2), claim(M, 5, 5), RESET);
}

/// Row 12: the sender claims a task the receiver knows no winner for.
#[test]
fn row_12() {
    row(12, claim(K, 5, 5), none(9), UPDATE);
}

/// Rows 13 to 16: the sender believes the receiver wins the task.
#[test]
fn rows_13_to_16() {
    row(13, claim(I, 5, 5), claim(I, 5, 5), QUIET);
    row(0, claim(I, 5, 2), claim(I, 5, 5), LEAVE);
    row(14, claim(I, 5, 5), claim(K, 5, 5), RESET);
    row(15, claim(I, 5, 5), claim(M, 5, 5), LEAVE);
    row(16, claim(I, 5, 5), none(2), LEAVE);
}

/// Rows 17 to 19: the sender believes a third agent wins, the receiver believes it does.
#[test]
fn rows_17_to_19() {
    row(17, claim(N, 9, 5), claim(I, 5, 5), UPDATE);
    row(18, claim(M, 5, 5), claim(I, 5, 5), UPDATE);
    row(0, claim(N, 5, 5), claim(I, 5, 5), LEAVE);
    row(19, claim(N, 4, 5), claim(I, 5, 5), UPDATE_TIME);
}

/// Rows 20 and 21: the sender believes a third agent wins, the receiver believes the sender
/// does; only a claim that is not older is taken.
#[test]
fn rows_20_and_21() {
    row(20, claim(M, 5, 5), claim(K, 9, 5), UPDATE);
    row(20, claim(M, 5, 7), claim(K, 9, 5), UPDATE);
    row(21, claim(M, 5, 4), claim(K, 9, 5), RESET);
}

/// Rows 22 to 24: both believe the same third agent wins; a newer time is taken silently.
#[test]
fn rows_22_to_24() {
    row(22, claim(M, 5, 9), claim(M, 5, 5), UPDATE_QUIETLY);
    row(23, claim(M, 5, 5), claim(M, 5, 5), QUIET);
    row(24, claim(M, 5, 2), claim(M, 5, 5), QUIET);
}

/// Rows 25 to 30: the sender and the receiver believe two different third agents win.
#[test]
fn rows_25_to_30() {
    row(25, claim(M, 9, 5), claim(N, 5, 5), UPDATE);
    row(26, claim(M, 4, 5), claim(N, 5, 5), LEAVE);
    row(27, claim(M, 5, 5), claim(N, 5, 5), LEAVE);
    row(28, claim(M, 4, 9), claim(N, 5, 5), RESET);
    row(29, claim(M, 9, 2), claim(N, 5, 5), RESET);
    row(30, claim(M, 5, 5), none(9), UPDATE);
}

/// Rows 31 to 34: the sender knows of no winner.
#[test]
fn rows_31_to_34() {
    row(31, none(9), claim(I, 5, 5), LEAVE);
    row(32, none(2), claim(K, 5, 5), UPDATE);
    row(33, none(9), claim(M, 5, 5), UPDATE);
    row(0, none(2), claim(M, 5, 5), LEAVE);
    row(34, none(9), none(5), QUIET);
}

/// A third party breaks a tie for the smaller winner only when asked to.
#[test]
fn third_party_ties_are_opt_in() {
    let breaking = Exchange {
        third_party_ties: true,
        ..exchange(I, K, 1)
    };

    assert_eq!(
        breaking.decide(claim(M, 5, 5), claim(N, 5, 5)),
        Decision {
            change: Change::Update,
            broadcast: Broadcast::Sender,
            row: 27
        }
    );
    assert_eq!(
        breaking.decide(claim(K, 5, 5), claim(M, 5, 5)).change,
        Change::Leave
    );
}

/// Times within `epsilon` count as equal, so a slightly newer copy of the same bid is quiet.
#[test]
fn epsilon_makes_close_times_equal() {
    let tolerant = exchange(I, K, 3);

    assert_eq!(tolerant.decide(claim(K, 5, 7), claim(K, 5, 5)).row, 5);
    assert_eq!(tolerant.decide(claim(K, 5, 8), claim(K, 5, 5)).row, 4);
}
