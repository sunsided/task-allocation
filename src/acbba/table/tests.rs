//! One test per row of Table 1, with receiver 1, sender 2 and third agents 0 and 3.
//!
//! Agent 0 is the smallest identifier, so ties can go either way.

use super::{Broadcast, Change, Claim, Decision, Exchange};
use crate::id::AgentId;

/// The receiver.
const I: AgentId = AgentId(1);
/// The sender.
const K: AgentId = AgentId(2);
/// A third agent, smaller than both.
const M: AgentId = AgentId(0);
/// Another third agent.
const N: AgentId = AgentId(3);

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

/// The decision for `sent` against `held`, with times equal below one tick.
fn decide(sent: Claim, held: Claim) -> Decision {
    Exchange {
        receiver: I,
        sender: K,
        epsilon: 1,
    }
    .decide(sent, held)
}

/// Checks that `sent` against `held` applies `row` with the given change and broadcast.
fn row(row: u8, sent: Claim, held: Claim, change: Change, broadcast: Broadcast) {
    assert_eq!(
        decide(sent, held),
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
    row(
        1,
        claim(K, 9, 5),
        claim(I, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        3,
        claim(K, 4, 5),
        claim(I, 5, 5),
        Change::UpdateTime,
        Broadcast::Own,
    );

    // Row 2 needs the sender to be the smaller agent; here it is not, so the default holds.
    row(
        0,
        claim(K, 5, 5),
        claim(I, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
    let smaller_sender = Exchange {
        receiver: K,
        sender: I,
        epsilon: 1,
    };
    assert_eq!(smaller_sender.decide(claim(I, 5, 5), claim(K, 5, 5)).row, 2);
}

/// Rows 4 to 6: both believe the sender wins; the newer claim counts.
#[test]
fn rows_4_to_6() {
    row(
        4,
        claim(K, 5, 7),
        claim(K, 3, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        5,
        claim(K, 5, 5),
        claim(K, 3, 5),
        Change::Leave,
        Broadcast::Nothing,
    );
    row(
        6,
        claim(K, 5, 4),
        claim(K, 3, 5),
        Change::Leave,
        Broadcast::Nothing,
    );
}

/// Rows 7 to 11: the sender claims the task, the receiver believes a third agent wins. Equal
/// bids (row 9) go to the smaller winner, a deviation from the paper.
#[test]
fn rows_7_to_11() {
    row(
        7,
        claim(K, 9, 5),
        claim(M, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        8,
        claim(K, 4, 5),
        claim(M, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
    row(
        9,
        claim(K, 5, 9),
        claim(M, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
    row(
        9,
        claim(K, 5, 9),
        claim(N, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        10,
        claim(K, 4, 6),
        claim(M, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        11,
        claim(K, 9, 4),
        claim(M, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
}

/// Row 12: the sender claims a task the receiver knows no winner for.
#[test]
fn row_12() {
    row(
        12,
        claim(K, 1, 5),
        none(9),
        Change::Update,
        Broadcast::Sender,
    );
}

/// Rows 13 to 16: the sender believes the receiver wins.
#[test]
fn rows_13_to_16() {
    row(
        13,
        claim(I, 5, 5),
        claim(I, 5, 5),
        Change::Leave,
        Broadcast::Nothing,
    );
    row(
        0,
        claim(I, 5, 4),
        claim(I, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
    row(
        14,
        claim(I, 5, 5),
        claim(K, 5, 5),
        Change::Reset,
        Broadcast::EmptyNow,
    );
    row(
        15,
        claim(I, 5, 5),
        claim(M, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
    row(
        16,
        claim(I, 5, 5),
        none(5),
        Change::Leave,
        Broadcast::EmptyNow,
    );
}

/// Rows 17 to 19: the sender believes a third agent wins, the receiver believes it wins.
#[test]
fn rows_17_to_19() {
    row(
        17,
        claim(N, 9, 5),
        claim(I, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        18,
        claim(M, 5, 5),
        claim(I, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        19,
        claim(N, 4, 5),
        claim(I, 5, 5),
        Change::UpdateTime,
        Broadcast::Own,
    );
    row(
        0,
        claim(N, 5, 5),
        claim(I, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
}

/// Row 20: the sender believes a third agent wins, the receiver believes the sender does.
#[test]
fn row_20() {
    row(
        20,
        claim(M, 1, 1),
        claim(K, 9, 9),
        Change::Update,
        Broadcast::Sender,
    );
}

/// Rows 22 to 24: both believe the same third agent wins; the newer claim counts.
#[test]
fn rows_22_to_24() {
    row(
        22,
        claim(M, 5, 7),
        claim(M, 3, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        23,
        claim(M, 5, 5),
        claim(M, 3, 5),
        Change::Leave,
        Broadcast::Nothing,
    );
    row(
        24,
        claim(M, 5, 4),
        claim(M, 3, 5),
        Change::Leave,
        Broadcast::Own,
    );
}

/// Rows 25 to 29: sender and receiver believe different third agents win. The table has no
/// row 27; equal bids go to the smaller winner and report as row 27.
#[test]
fn rows_25_to_29() {
    row(
        25,
        claim(M, 9, 5),
        claim(N, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        26,
        claim(M, 4, 5),
        claim(N, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
    row(
        27,
        claim(M, 5, 9),
        claim(N, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        27,
        claim(N, 5, 9),
        claim(M, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
    row(
        28,
        claim(M, 4, 6),
        claim(N, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        29,
        claim(M, 9, 4),
        claim(N, 5, 5),
        Change::Leave,
        Broadcast::Own,
    );
}

/// Row 30: the sender believes a third agent wins a task the receiver knows no winner for.
#[test]
fn row_30() {
    row(
        30,
        claim(M, 1, 1),
        none(9),
        Change::Update,
        Broadcast::Sender,
    );
}

/// Rows 31 to 34: the sender knows no winner.
#[test]
fn rows_31_to_34() {
    row(31, none(9), claim(I, 5, 5), Change::Leave, Broadcast::Own);
    row(
        32,
        none(1),
        claim(K, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(
        33,
        none(6),
        claim(M, 5, 5),
        Change::Update,
        Broadcast::Sender,
    );
    row(0, none(4), claim(M, 5, 5), Change::Leave, Broadcast::Own);
    row(34, none(4), none(5), Change::Leave, Broadcast::Nothing);
}

/// A time within `epsilon` counts as equal, even when it is later.
#[test]
fn epsilon_equality_wins_over_newer() {
    let tolerant = Exchange {
        receiver: I,
        sender: K,
        epsilon: 3,
    };

    assert_eq!(tolerant.decide(claim(K, 5, 7), claim(K, 3, 5)).row, 5);
}
