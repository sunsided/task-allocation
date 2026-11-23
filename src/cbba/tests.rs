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

/// A message reports the sender's view of every task.
#[test]
fn message_exposes_the_senders_view() {
    let mut sender = agent(1, 2, 1, &[5, 0]);
    sender.build_bundle();
    let message = sender.message();

    assert_eq!(message.sender(), AgentId(1));
    assert_eq!(message.tasks(), 2);
    assert_eq!(message.winner(TaskId(0)), Some(AgentId(1)));
    assert_eq!(message.winning_bid(TaskId(0)), 5);
    assert_eq!(message.winner(TaskId(1)), None);
}

/// When the sender believes the receiver wins a task the receiver believes the sender wins,
/// both beliefs are stale, and the receiver forgets the winner.
#[test]
fn crossed_beliefs_reset_the_task() {
    let mut receiver = agent(0, 2, 1, &[0]);
    receiver.claims[0] = super::Claim {
        winner: Some(AgentId(1)),
        bid: 5,
    };
    let message = super::Message {
        sender: AgentId(1),
        claims: alloc::vec![super::Claim {
            winner: Some(AgentId(0)),
            bid: 5,
        }],
        seen: alloc::vec![0, 1],
    };

    assert!(receiver.receive(&message));
    assert_eq!(receiver.winner(TaskId(0)), None);
    assert_eq!(receiver.winning_bid(TaskId(0)), 0);
}

/// Replanning releases the last tasks, or all of them, and rebuilds against the current
/// winners.
#[test]
fn replanning_rebuilds_released_tasks() {
    let mut keep = agent(0, 1, 3, &[9, 8, 7]);
    let mut last = agent(0, 1, 3, &[9, 8, 7]).with_replan(super::Replan::Last(1));
    let mut full = agent(0, 1, 3, &[9, 8, 7]).with_replan(super::Replan::Full);

    assert_eq!(
        (
            keep.build_bundle(),
            last.build_bundle(),
            full.build_bundle()
        ),
        (3, 3, 3)
    );
    assert_eq!(
        (
            keep.build_bundle(),
            last.build_bundle(),
            full.build_bundle()
        ),
        (0, 1, 3)
    );
    assert_eq!(bundle_of(&full), [0, 1, 2]);
    assert!(!keep.can_bid() && !last.can_bid() && !full.can_bid());
}

/// A bundle budget caps the tasks one bundle phase adds, and the bundle fills over rounds.
#[test]
fn bundle_budget_spreads_the_phase() {
    let mut budgeted = agent(0, 1, 3, &[9, 8, 7]).with_bundle_budget(2);

    assert_eq!(budgeted.build_bundle(), 2);
    assert!(budgeted.can_bid());
    assert_eq!(budgeted.build_bundle(), 1);
    assert_eq!(bundle_of(&budgeted), [0, 1, 2]);
    assert!(!budgeted.can_bid());
}

/// A budget that allows no task is a caller bug.
#[test]
#[should_panic(expected = "allowed to add a task")]
fn zero_bundle_budget_panics() {
    let _ = agent(0, 1, 1, &[1]).with_bundle_budget(0);
}

/// A replan that releases more than a budget below the capacity re-adds is rejected.
#[test]
#[should_panic(expected = "cannot re-add what this replan releases")]
fn full_replan_outruns_a_small_budget() {
    let _ = agent(0, 1, 3, &[9, 8, 7])
        .with_replan(super::Replan::Full)
        .with_bundle_budget(2);
}

/// A partial replan releasing exactly the budget would keep the bundle from growing; the
/// check also holds when the budget is set first.
#[test]
#[should_panic(expected = "cannot re-add what this replan releases")]
fn partial_replan_matching_the_budget_is_rejected() {
    let _ = agent(0, 1, 3, &[9, 8, 7])
        .with_bundle_budget(2)
        .with_replan(super::Replan::Last(2));
}

/// A partial replan within the budget still fills the bundle, and a budget that covers the
/// capacity works with any replan.
#[test]
fn budgets_that_cover_the_replan_are_fine() {
    let mut partial = agent(0, 1, 3, &[9, 8, 7])
        .with_bundle_budget(2)
        .with_replan(super::Replan::Last(1));
    partial.build_bundle();
    partial.build_bundle();
    assert_eq!(partial.bundle().len(), 3);

    let mut full = agent(0, 1, 3, &[9, 8, 7])
        .with_bundle_budget(3)
        .with_replan(super::Replan::Full);
    full.build_bundle();
    assert_eq!(full.bundle().len(), 3);
}

/// A task added to a running agent is bid on with the room left; the bundle stays.
#[test]
fn added_tasks_are_bid_on_with_room_left() {
    let valuation = Rewards {
        rewards: alloc::vec![9, 8, 20],
        step: 1,
    };
    let mut worker = Agent::new(AgentId(0), 1, 2, 3, valuation);
    worker.build_bundle();
    assert_eq!(bundle_of(&worker), [0, 1]);

    let task = worker.add_task();
    worker.build_bundle();

    assert_eq!(task, TaskId(2));
    assert_eq!(bundle_of(&worker), [0, 1, 2]);
}

/// A message about fewer tasks than the receiver knows merges over the tasks both know.
#[test]
fn shorter_messages_merge_over_shared_tasks() {
    let mut receiver = agent(0, 2, 1, &[1, 1, 1]);
    let mut sender = agent(1, 2, 1, &[0, 9]);
    sender.build_bundle();

    assert!(receiver.receive(&sender.message()));
    assert_eq!(receiver.winner(TaskId(1)), Some(AgentId(1)));
    assert_eq!(receiver.winner(TaskId(2)), None);
}

/// A retired task is given up with the tasks added after it, and never bid on again, even
/// when a stale message still names a winner.
#[test]
fn retired_tasks_are_released_and_stay_void() {
    let mut worker = agent(0, 2, 3, &[9, 8, 7]);
    let mut other = agent(1, 2, 1, &[0, 20, 0]);
    worker.build_bundle();
    other.build_bundle();
    let stale = other.message();

    assert!(worker.retire_task(TaskId(1)));
    assert_eq!(bundle_of(&worker), [0]);

    worker.receive(&stale);
    worker.build_bundle();
    assert_eq!(worker.winner(TaskId(1)), None);
    assert_eq!(bundle_of(&worker), [0, 2]);
}

/// An agent that leaves gives up everything; the others void its claims and its messages.
#[test]
fn departed_agents_are_forgotten() {
    let mut stays = agent(0, 2, 1, &[5, 5]);
    let mut goes = agent(1, 2, 1, &[0, 9]);
    goes.build_bundle();
    let goodbye = goes.message();
    stays.receive(&goodbye);
    assert_eq!(stays.winner(TaskId(1)), Some(AgentId(1)));

    goes.leave();
    stays.forget_agent(AgentId(1));

    assert!(goes.has_left() && goes.bundle().is_empty() && !goes.can_bid());
    assert!(stays.has_departed(AgentId(1)));
    assert_eq!(stays.winner(TaskId(1)), None);
    assert!(!stays.receive(&goodbye));
}
