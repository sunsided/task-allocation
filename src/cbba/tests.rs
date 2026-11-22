//! Unit tests for [`Agent`]: bundle phase, consensus phase and stale messages.

use alloc::vec::Vec;

use super::Agent;
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;

/// Values task `t` at `rewards[t]`, minus `step` for every task already held.
struct Rewards {
    /// The value of each task to an agent holding nothing.
    rewards: Vec<u64>,
    /// How much every task already held lowers the next one's value.
    step: u64,
}

impl Valuation<TaskId> for Rewards {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        let held = u64::try_from(bundle.len()).ok()?;

        self.rewards[task.index()].checked_sub(self.step * held)
    }
}

/// Agent `id` of a team of `agents`, holding up to `capacity` tasks valued at `rewards`.
fn agent(id: usize, agents: usize, capacity: usize, rewards: &[u64]) -> Agent<Rewards> {
    Agent::new(
        AgentId(id),
        agents,
        rewards.len(),
        capacity,
        Rewards {
            rewards: rewards.to_vec(),
            step: 1,
        },
    )
}

/// The tasks in `agent`'s bundle, in the order it added them.
fn bundle_of(agent: &Agent<Rewards>) -> Vec<usize> {
    agent.bundle().tasks().map(|task| task.index()).collect()
}

/// The bundle phase takes the best tasks, bids their marginal gains and claims them.
#[test]
fn bundle_phase_claims_the_best_tasks() {
    let mut alone = agent(0, 1, 2, &[3, 9, 5]);

    assert_eq!(alone.build_bundle(), 2);
    assert_eq!(bundle_of(&alone), [1, 2]);
    assert_eq!(alone.winning_bid(TaskId(2)), 4);
    assert_eq!(alone.winner(TaskId(0)), None);
    assert!(alone.is_consistent());
    assert_eq!(alone.build_bundle(), 0);
}

/// An outbid agent drops the lost task and everything it added after it, then rebuilds.
#[test]
fn outbid_agent_releases_later_tasks() {
    let mut low = agent(0, 2, 3, &[9, 8, 7]);
    let mut high = agent(1, 2, 1, &[1, 20, 1]);
    low.build_bundle();
    high.build_bundle();

    assert!(low.receive(&high.message()));
    assert_eq!(bundle_of(&low), [0]);
    assert_eq!(low.winner(TaskId(1)), Some(AgentId(1)));
    assert_eq!(low.winner(TaskId(2)), None);

    low.build_bundle();
    assert_eq!(bundle_of(&low), [0, 2]);
}

/// A message older than what the receiver has already seen of the sender is ignored.
#[test]
fn stale_messages_are_ignored() {
    let mut receiver = agent(0, 2, 1, &[1, 1]);
    let mut sender = agent(1, 2, 1, &[5, 0]);
    let before_bidding = sender.message();
    sender.build_bundle();
    let after_bidding = sender.message();

    assert!(receiver.receive(&after_bidding));
    assert!(!receiver.receive(&before_bidding));
    assert_eq!(receiver.winner(TaskId(0)), Some(AgentId(1)));
    assert_eq!(after_bidding.clock(), 2);
}

/// Messages from the agent itself or from a differently sized team are ignored.
#[test]
fn foreign_messages_are_ignored() {
    let mut a = agent(0, 2, 1, &[1, 1]);
    let mut other_team = agent(1, 3, 1, &[5, 5]);
    other_team.build_bundle();
    let own = a.message();

    assert!(!a.receive(&own));
    assert!(!a.receive(&other_team.message()));
}

/// Equal bids go to the smaller agent.
#[test]
fn ties_go_to_the_smaller_agent() {
    let mut a = agent(0, 2, 1, &[5]);
    let mut b = agent(1, 2, 1, &[5]);
    a.build_bundle();
    b.build_bundle();

    b.receive(&a.message());
    a.receive(&b.message());

    assert_eq!(bundle_of(&a), [0]);
    assert_eq!(bundle_of(&b), Vec::<usize>::new());
    assert_eq!(b.winner(TaskId(0)), Some(AgentId(0)));
}

/// An agent outside the team cannot be created.
#[test]
#[should_panic(expected = "is not part of a team of 2 agents")]
fn identifier_must_fit_the_team() {
    let _ = agent(2, 2, 1, &[1]);
}
