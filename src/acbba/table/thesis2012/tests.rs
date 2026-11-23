//! Checks the rows the thesis marks, and that the others follow the 2011 table.

use super::super::{Broadcast, Change, Claim, Decision, Exchange, Rules};
use crate::id::AgentId;

/// The receiver.
const I: AgentId = AgentId(1);
/// The sender.
const K: AgentId = AgentId(2);
/// A third agent.
const M: AgentId = AgentId(0);

/// A claim that `winner` wins with `bid`, made at `time`.
const fn claim(winner: AgentId, bid: u64, time: u64) -> Claim {
    Claim {
        winner: Some(winner),
        bid,
        time,
    }
}

/// The decision for `sent` against `held` under `rules`.
fn decide(rules: Rules, sent: Claim, held: Claim) -> Decision {
    Exchange {
        receiver: I,
        sender: K,
        epsilon: 1,
        rules,
        third_party_ties: true,
    }
    .decide(sent, held)
}

/// Checks that `sent` against `held` applies the marked `row` with `change` and
/// `broadcast`.
fn marked(row: u8, sent: Claim, held: Claim, change: Change, broadcast: Broadcast) {
    let expected = Decision {
        change,
        broadcast,
        row,
    };

    assert_eq!(decide(Rules::Thesis2012, sent, held), expected, "row {row}");
}

/// Marked updates stamp the sender's claim past the receiver's.
#[test]
fn marked_updates_push_the_senders_time() {
    marked(
        1,
        claim(K, 9, 5),
        claim(I, 5, 5),
        Change::UpdatePast,
        Broadcast::Own,
    );
    marked(
        17,
        claim(M, 9, 5),
        claim(I, 5, 5),
        Change::UpdatePast,
        Broadcast::Own,
    );
}

/// Marked leaves stamp the receiver's entry past the sender's; row 32 keeps the
/// receiver's entry, as tabulated.
#[test]
fn marked_leaves_push_the_receivers_time() {
    let none = Claim::empty_at(9);

    marked(
        3,
        claim(K, 4, 5),
        claim(I, 5, 5),
        Change::KeepPast,
        Broadcast::Own,
    );
    marked(
        13,
        claim(I, 5, 5),
        claim(I, 5, 5),
        Change::KeepPast,
        Broadcast::Nothing,
    );
    marked(
        15,
        claim(I, 5, 5),
        claim(M, 5, 5),
        Change::KeepPast,
        Broadcast::Own,
    );
    marked(
        19,
        claim(M, 4, 5),
        claim(I, 5, 5),
        Change::KeepPast,
        Broadcast::Own,
    );
    marked(31, none, claim(I, 5, 5), Change::KeepPast, Broadcast::Own);
    marked(32, none, claim(K, 5, 5), Change::KeepPast, Broadcast::Own);
}

/// Unmarked rows are the 2011 table's.
#[test]
fn other_rows_follow_2011() {
    let cases = [
        (claim(K, 5, 9), claim(K, 5, 5)),
        (claim(K, 4, 9), claim(M, 5, 5)),
        (claim(I, 5, 5), claim(K, 5, 5)),
        (claim(M, 5, 9), claim(K, 5, 5)),
        (Claim::empty_at(9), claim(M, 5, 5)),
    ];

    for (sent, held) in cases {
        assert_eq!(
            decide(Rules::Thesis2012, sent, held),
            decide(Rules::Infotech2011, sent, held)
        );
    }
}
