//! Unit tests for [`Agent`].

use alloc::vec;
use alloc::vec::Vec;

use super::{Agent, Bid, Entry, Packet};
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;

/// Values task `t` at `rewards[t]`, minus one per task already held.
#[derive(Clone, Debug)]
struct Rewards(Vec<u64>);

impl Valuation<TaskId> for Rewards {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        let held = u64::try_from(bundle.len()).expect("small bundle");

        self.0[task.index()].checked_sub(held)
    }
}

/// Agent `id` of a team of three, holding up to `capacity` tasks valued at `rewards`.
fn agent(id: usize, capacity: usize, rewards: &[u64]) -> Agent<Rewards> {
    Agent::new(
        AgentId(id),
        3,
        rewards.len(),
        capacity,
        Rewards(rewards.to_vec()),
    )
}

/// A bid by `bidder` of `gain` on `task`, made at `time`.
const fn bid(task: usize, bidder: usize, gain: u64, time: u64) -> Entry {
    Entry::Bid {
        task: TaskId(task),
        bid: Bid {
            bidder: AgentId(bidder),
            gain,
        },
        time,
    }
}

/// A packet from `sender` with `entries`.
fn packet(sender: usize, entries: Vec<Entry>) -> Packet {
    Packet {
        sender: AgentId(sender),
        entries,
    }
}

/// The bundle phase queues one bid per new task, stamped after any drop of the same round.
#[test]
fn new_bids_are_queued() {
    let mut alone = agent(0, 1, &[3, 9]);
    alone.build_bundle(4);

    assert_eq!(alone.packet(), Some(packet(0, vec![bid(1, 0, 9, 10)])));
    alone.build_bundle(5);
    assert_eq!(alone.packet(), None);
}

/// An outbid agent releases the task and announces a drop bid in its next round.
#[test]
fn outbid_agents_announce_drops() {
    let mut low = agent(1, 1, &[5]);
    low.build_bundle(0);
    let _ = low.packet();

    low.receive(&packet(0, vec![bid(0, 0, 8, 2)]));
    assert!(low.bundle().is_empty());
    low.build_bundle(1);

    let entries = low.packet().expect("queued").entries;
    assert!(entries.contains(&bid(0, 0, 8, 2)));
    assert!(entries.contains(&Entry::Drop {
        task: TaskId(0),
        bidder: AgentId(1),
        time: 3,
    }));
}

/// After a drop, an older copy of the dropped bid is ignored, however it arrives.
#[test]
fn stale_copies_of_dropped_bids_are_ignored() {
    let mut relay = agent(2, 1, &[0]);
    relay.receive(&packet(
        1,
        vec![Entry::Drop {
            task: TaskId(0),
            bidder: AgentId(0),
            time: 5,
        }],
    ));
    relay.receive(&packet(1, vec![bid(0, 0, 8, 2)]));

    assert_eq!(relay.best(TaskId(0)), None);
}

/// A bid that does not beat the known one is answered with the known bid and its time.
#[test]
fn weaker_bids_are_answered() {
    let mut holder = agent(0, 1, &[9]);
    holder.build_bundle(0);
    let _ = holder.packet();

    holder.receive(&packet(1, vec![bid(0, 1, 4, 4)]));

    assert_eq!(holder.packet(), Some(packet(0, vec![bid(0, 0, 9, 2)])));
    assert!(holder.is_consistent());
}

/// A task released and taken back with the same gain still counts as won.
#[test]
fn retaken_tasks_stay_consistent() {
    let mut agent = agent(1, 2, &[9, 9]);
    agent.build_bundle(0);
    let _ = agent.packet();

    agent.receive(&packet(0, vec![bid(0, 0, 20, 2)]));
    agent.build_bundle(1);

    assert!(agent.is_consistent());
}
