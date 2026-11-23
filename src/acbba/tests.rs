//! Unit tests for [`Agent`].

use alloc::vec::Vec;

use super::{Agent, Claim, Message};
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;

/// Values task `t` at `rewards[t]`, minus one for every task already held.
struct Rewards(Vec<u64>);

impl Valuation<TaskId> for Rewards {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        let held = u64::try_from(bundle.len()).ok()?;

        self.0[task.index()].checked_sub(held)
    }
}

/// Agent `id`, holding up to `capacity` tasks valued at `rewards`.
fn agent(id: usize, capacity: usize, rewards: &[u64]) -> Agent<Rewards> {
    Agent::new(
        AgentId(id),
        rewards.len(),
        capacity,
        Rewards(rewards.to_vec()),
    )
}

/// The tasks in `agent`'s bundle, in the order it added them.
fn bundle_of(agent: &Agent<Rewards>) -> Vec<usize> {
    agent.bundle().tasks().map(|task| task.index()).collect()
}

/// The bundle phase announces each new bid with the current time.
#[test]
fn new_bids_are_announced() {
    let mut alone = agent(0, 2, &[3, 9, 5]);
    let announcements = alone.build_bundle(7);

    assert_eq!(bundle_of(&alone), [1, 2]);
    assert_eq!(announcements.len(), 2);
    assert_eq!(
        announcements[1],
        Message {
            sender: AgentId(0),
            task: TaskId(2),
            claim: Claim {
                winner: Some(AgentId(0)),
                bid: 4,
                time: 7
            }
        }
    );
    assert_eq!(alone.build_bundle(8), []);
    assert!(!alone.can_bid());
}

/// An outbid agent takes the higher bid, passes it on, drops later tasks and announces
/// those as empty bids.
#[test]
fn outbid_agent_releases_and_announces() {
    let mut low = agent(0, 3, &[9, 8, 7]);
    let mut high = agent(1, 1, &[1, 20, 1]);
    low.build_bundle(1);
    let bids = high.build_bundle(1);

    let answers = low.receive(2, &bids[0]);

    assert_eq!(bundle_of(&low), [0]);
    assert_eq!(low.claim(TaskId(1)).winner, Some(AgentId(1)));
    assert_eq!(low.claim(TaskId(2)), Claim::empty_at(2));
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0].claim, bids[0].claim);
    assert_eq!(answers[1].task, TaskId(2));
    assert!(low.is_consistent());
}

/// A lower bid on a task the receiver wins makes it confirm its own bid with the current
/// time.
#[test]
fn lower_bids_are_answered_with_a_confirmation() {
    let mut winner = agent(0, 1, &[9]);
    let mut loser = agent(1, 1, &[5]);
    winner.build_bundle(1);
    let bid = loser.build_bundle(1);

    let answers = winner.receive(4, &bid[0]);

    assert_eq!(winner.claim(TaskId(0)).time, 4);
    assert_eq!(answers[0].claim.winner, Some(AgentId(0)));
}

/// Messages from the agent itself or about unknown tasks are ignored.
#[test]
fn foreign_messages_are_ignored() {
    let mut a = agent(0, 1, &[1]);
    let own = Message {
        sender: AgentId(0),
        task: TaskId(0),
        claim: Claim::NONE,
    };
    let unknown = Message {
        sender: AgentId(1),
        task: TaskId(9),
        claim: Claim::NONE,
    };

    assert_eq!(a.receive(1, &own), []);
    assert_eq!(a.receive(1, &unknown), []);
}

/// A zero announcement period is a caller bug.
#[test]
#[should_panic(expected = "must be positive")]
fn zero_period_panics() {
    let _ = agent(0, 1, &[1]).announce_every(0);
}

/// A tie between two other winners goes to the smaller one, unless the agent leaves ties as
/// printed.
#[test]
fn third_party_ties_follow_the_agent_setting() {
    let tied = |sender: usize, winner: usize| Message {
        sender: AgentId(sender),
        task: TaskId(0),
        claim: Claim {
            winner: Some(AgentId(winner)),
            bid: 5,
            time: 3,
        },
    };

    for (printed, expected) in [(false, 0), (true, 3)] {
        let mut onlooker = agent(1, 1, &[0]);
        if printed {
            onlooker = onlooker.with_printed_ties();
        }

        onlooker.receive(3, &tied(3, 3));
        onlooker.receive(4, &tied(2, 0));

        assert_eq!(onlooker.claim(TaskId(0)).winner, Some(AgentId(expected)));
    }
}
