//! Unit tests for bid warping in [`Agent`]'s bundle phase.

use alloc::vec::Vec;

use crate::cbba::Agent;
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;

/// Values task `t` at `rewards[t]`, plus `step` for every task already held: gains grow.
struct Growing {
    /// The value of each task to an agent holding nothing.
    rewards: Vec<u64>,

    /// How much every task already held raises the next one's value.
    step: u64,
}

impl Valuation<TaskId> for Growing {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        let held = u64::try_from(bundle.len()).ok()?;

        Some(self.rewards[task.index()] + self.step * held)
    }
}

/// Agent 1 of a team of two, holding up to three tasks worth 10, 8 and 5 on their own and
/// 5 more for every task held, having heard that agent 0 bids 10 on task 1.
fn outbid_on_task_one() -> Agent<Growing> {
    let mut rival = Agent::new(
        AgentId(0),
        2,
        3,
        1,
        Growing {
            rewards: alloc::vec![0, 10, 0],
            step: 0,
        },
    );
    rival.build_bundle();

    let mut agent = Agent::new(
        AgentId(1),
        2,
        3,
        3,
        Growing {
            rewards: alloc::vec![10, 8, 5],
            step: 5,
        },
    );
    agent.receive(&rival.message());

    agent
}

/// The tasks in `agent`'s bundle with their bids, in the order it added them.
fn entries(agent: &Agent<Growing>) -> Vec<(usize, u64)> {
    agent
        .bundle()
        .entries()
        .map(|(task, bid)| (task.index(), bid))
        .collect()
}

/// Without warping, the agent outbids agent 0 on task 1 with its grown gain of 13.
#[test]
fn unwarped_bids_grow() {
    let mut agent = outbid_on_task_one();
    agent.build_bundle();

    assert_eq!(entries(&agent), [(0, 10), (1, 13), (2, 15)]);
}

/// With warping, task 1's bid is capped at 10 and loses the tie to agent 0, so the agent
/// takes task 2, worth less but winnable at 10. With room left, it sees nothing more to
/// bid on, although task 1's unwarped gain of 18 would now win.
#[test]
fn warped_bids_skip_tasks_they_cannot_win() {
    let mut agent = outbid_on_task_one().with_bid_warping();
    agent.build_bundle();

    assert_eq!(entries(&agent), [(0, 10), (2, 10)]);
    assert_eq!(agent.winning_bid(TaskId(2)), 10);
    assert!(agent.is_consistent());
    assert!(!agent.can_bid());
}
