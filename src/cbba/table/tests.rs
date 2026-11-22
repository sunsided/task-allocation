//! Row-by-row tests of the conflict-resolution table, with receiver 0, sender 1 and third
//! agents 2 and 3.

use super::{Action, Claim, Exchange};
use crate::id::AgentId;

/// The receiver.
const I: AgentId = AgentId(0);
/// The sender.
const K: AgentId = AgentId(1);
/// A third agent.
const M: AgentId = AgentId(2);
/// Another third agent.
const N: AgentId = AgentId(3);

/// A claim that `winner` wins with `bid`.
const fn claim(winner: AgentId, bid: u64) -> Claim {
    Claim {
        winner: Some(winner),
        bid,
    }
}

/// Resolves `sent` against `held`, with the given freshness tables over agents 0 to 3.
fn resolve(sent: Claim, held: Claim, sender_seen: [u64; 4], receiver_seen: [u64; 4]) -> Action {
    Exchange {
        receiver: I,
        sender: K,
        sender_seen: &sender_seen,
        receiver_seen: &receiver_seen,
    }
    .resolve(sent, held)
}

/// Both sides have seen the same of everyone.
const SAME: [u64; 4] = [5, 5, 5, 5];
/// The sender has seen more of agent 2.
const SENDER_KNOWS_M: [u64; 4] = [5, 5, 6, 5];
/// The sender has seen more of agent 3.
const SENDER_KNOWS_N: [u64; 4] = [5, 5, 5, 6];
/// The sender has seen more of agents 2 and 3.
const SENDER_KNOWS_BOTH: [u64; 4] = [5, 5, 6, 6];
/// The receiver has seen more of agent 2.
const RECEIVER_KNOWS_M: [u64; 4] = [5, 5, 6, 5];

/// Higher bids win, and equal bids go to the smaller agent.
#[test]
fn outbidding_breaks_ties_by_agent() {
    assert!(claim(K, 5).outbids(claim(I, 4)));
    assert!(!claim(K, 5).outbids(claim(I, 5)));
    assert!(claim(I, 5).outbids(claim(K, 5)));
    assert!(claim(K, 1).outbids(Claim::NONE));
    assert!(!Claim::NONE.outbids(claim(K, 1)));
}

/// The sender claims the task for itself.
#[test]
fn sender_claims_itself() {
    assert_eq!(
        resolve(claim(K, 5), claim(I, 4), SAME, SAME),
        Action::Update
    );
    assert_eq!(resolve(claim(K, 4), claim(I, 5), SAME, SAME), Action::Leave);
    assert_eq!(
        resolve(claim(K, 1), claim(K, 9), SAME, SAME),
        Action::Update
    );
    assert_eq!(
        resolve(claim(K, 1), claim(M, 9), SENDER_KNOWS_M, SAME),
        Action::Update
    );
    assert_eq!(
        resolve(claim(K, 9), claim(M, 1), SAME, SAME),
        Action::Update
    );
    assert_eq!(resolve(claim(K, 1), claim(M, 9), SAME, SAME), Action::Leave);
    assert_eq!(
        resolve(claim(K, 1), Claim::NONE, SAME, SAME),
        Action::Update
    );
}

/// The sender believes the receiver wins.
#[test]
fn sender_claims_receiver() {
    assert_eq!(resolve(claim(I, 5), claim(I, 5), SAME, SAME), Action::Leave);
    assert_eq!(resolve(claim(I, 5), claim(K, 5), SAME, SAME), Action::Reset);
    assert_eq!(
        resolve(claim(I, 5), claim(M, 5), SENDER_KNOWS_M, SAME),
        Action::Reset
    );
    assert_eq!(resolve(claim(I, 5), claim(M, 5), SAME, SAME), Action::Leave);
    assert_eq!(resolve(claim(I, 5), Claim::NONE, SAME, SAME), Action::Leave);
}

/// The sender believes a third agent wins, and the receiver believes it wins itself, the
/// sender wins, the same third agent wins, or nobody does.
#[test]
fn sender_claims_a_third_agent() {
    assert_eq!(
        resolve(claim(M, 9), claim(I, 5), SENDER_KNOWS_M, SAME),
        Action::Update
    );
    assert_eq!(resolve(claim(M, 9), claim(I, 5), SAME, SAME), Action::Leave);
    assert_eq!(
        resolve(claim(M, 1), claim(I, 5), SENDER_KNOWS_M, SAME),
        Action::Leave
    );

    assert_eq!(
        resolve(claim(M, 1), claim(K, 5), SENDER_KNOWS_M, SAME),
        Action::Update
    );
    assert_eq!(resolve(claim(M, 1), claim(K, 5), SAME, SAME), Action::Reset);

    assert_eq!(
        resolve(claim(M, 1), claim(M, 5), SENDER_KNOWS_M, SAME),
        Action::Update
    );
    assert_eq!(resolve(claim(M, 1), claim(M, 5), SAME, SAME), Action::Leave);

    assert_eq!(
        resolve(claim(M, 1), Claim::NONE, SENDER_KNOWS_M, SAME),
        Action::Update
    );
    assert_eq!(resolve(claim(M, 1), Claim::NONE, SAME, SAME), Action::Leave);
}

/// The sender and the receiver believe two different third agents win.
#[test]
fn two_third_parties() {
    assert_eq!(
        resolve(claim(M, 1), claim(N, 5), SENDER_KNOWS_BOTH, SAME),
        Action::Update
    );
    assert_eq!(
        resolve(claim(M, 9), claim(N, 5), SENDER_KNOWS_M, SAME),
        Action::Update
    );
    assert_eq!(
        resolve(claim(M, 1), claim(N, 5), SENDER_KNOWS_M, SAME),
        Action::Leave
    );
    assert_eq!(
        resolve(claim(M, 9), claim(N, 5), SENDER_KNOWS_N, RECEIVER_KNOWS_M),
        Action::Reset
    );
    assert_eq!(
        resolve(claim(M, 1), claim(N, 5), SENDER_KNOWS_N, SAME),
        Action::Update
    );
    assert_eq!(resolve(claim(M, 9), claim(N, 5), SAME, SAME), Action::Leave);
}

/// The sender knows of no winner.
#[test]
fn sender_claims_nobody() {
    assert_eq!(resolve(Claim::NONE, claim(I, 5), SAME, SAME), Action::Leave);
    assert_eq!(
        resolve(Claim::NONE, claim(K, 5), SAME, SAME),
        Action::Update
    );
    assert_eq!(
        resolve(Claim::NONE, claim(M, 5), SENDER_KNOWS_M, SAME),
        Action::Update
    );
    assert_eq!(resolve(Claim::NONE, claim(M, 5), SAME, SAME), Action::Leave);
    assert_eq!(resolve(Claim::NONE, Claim::NONE, SAME, SAME), Action::Leave);
}
