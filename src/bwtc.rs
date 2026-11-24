//! The task consensus of bid-warped CBBA (BW-TC): agents bid as in CBBA, but exchange
//! only what changed, and announce every bid they withdraw.
//!
//! **Experimental.** This is a candidate replacement for [`acbba`](crate::acbba)'s
//! deconfliction tables, which do not settle reliably. Its published convergence proof
//! assumes synchronous, reliable rounds.
//!
//! Each agent keeps the best bid it knows for every task, and a timestamp for every pair of
//! bidder and task. After its bundle phase, it compares its bundle with the one it last
//! announced: tasks it no longer holds go out as *drop bids*, new tasks as bids. A receiver
//! takes an entry only when it is newer than what it has seen of that bidder on that task,
//! so a withdrawn bid can never come back from a stale copy, however it is relayed. A newer
//! drop removes the bidder's bid; a newer bid is kept, and passed on when it is the best
//! known, while otherwise the receiver answers with the best bid it knows. Everything an agent learns or
//! answers goes out together at the end of its next round.
//!
//! The consensus is Algorithms 4 to 6 of Johnson, Choi, Ponda and How, "Decentralized Task
//! Allocation Using Local Information Consistency Assumptions", Journal of Aerospace
//! Information Systems 14(2), 2017 (<https://doi.org/10.2514/1.I010461>). The paper also
//! warps bids so that scores need not diminish; this module does not, so valuations must
//! have [diminishing marginal gains](crate::check_diminishing), as for CBBA.
//!
//! Choices this implementation makes where the paper leaves room:
//!
//! - Timestamps come from a clock all agents share, passed in as `now`, counted in half
//!   ticks: a drop made at tick `t` is stamped `2t + 1` and a bid `2t + 2`, so a bid always
//!   comes after a drop the same agent made in the same round. This is the paper's `δ`.
//! - A bid replaces a task's entry in an agent's bundle only when the bid changed; a task
//!   kept with the same gain is not announced again.
//! - Nothing is ever repeated, so a lost message is lost for good: the paper says as much.
//!   Use it on networks that deliver every message.
//!
//! One deliberate deviation: the paper keeps one bid per task, the best known, and records
//! the time of every bid it hears, kept or not. An agent that hears a bid while a better
//! one stands then discards it, and when the better bid is dropped and the agent bids on
//! the free task itself, the others' answers carry the discarded bid at a time it has
//! already seen, so it ignores them: the team falls quiet with the agent believing it wins
//! a task another agent holds. Here an agent keeps the latest bid of every bidder on every
//! task, and the best of them counts. Since every withdrawal is announced, a kept bid stays
//! valid until its drop arrives, and the runner-up takes over when the best bid is dropped;
//! the agent then announces the runner-up along with the drop, since its neighbors may
//! never have heard of it.

use alloc::vec;
use alloc::vec::Vec;

use crate::bundle::Bundle;
use crate::id::{AgentId, TaskId};
use crate::sim::Participant;
use crate::valuation::Valuation;

/// A bid: who placed it, and its gain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bid {
    /// The agent that placed it.
    pub bidder: AgentId,

    /// Its marginal gain; positive.
    pub gain: u64,
}

impl Bid {
    /// Whether this bid beats `other`: a higher gain, or an equal gain by a smaller agent.
    #[must_use]
    pub fn outbids(self, other: Self) -> bool {
        self.gain > other.gain || (self.gain == other.gain && self.bidder < other.bidder)
    }
}

/// One piece of news about one task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Entry {
    /// A bid, made at `time`.
    Bid {
        /// The task bid on.
        task: TaskId,

        /// The bid.
        bid: Bid,

        /// When the bidder placed it, in half ticks.
        time: u64,
    },

    /// A bidder's withdrawal of its bid on a task, made at `time`.
    Drop {
        /// The task.
        task: TaskId,

        /// The agent withdrawing its bid.
        bidder: AgentId,

        /// When it withdrew it, in half ticks.
        time: u64,
    },
}

/// Everything an agent broadcasts at the end of one round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    /// The agent that broadcast it.
    pub sender: AgentId,

    /// What it learned or answered since its last broadcast, in order.
    pub entries: Vec<Entry>,
}

/// One BW-TC agent: its valuation, its bundle, and the best bid it knows for every task.
///
/// # Examples
///
/// ```
/// use task_allocation::bwtc::Agent;
/// use task_allocation::sim::{Network, Simulation};
/// use task_allocation::{AgentId, TaskId, Valuation};
///
/// /// Values task `t` at `10 + t` for agent 0 and `20 - t` for agent 1, minus 2 per task held.
/// struct Skewed(u64);
///
/// impl Valuation<TaskId> for Skewed {
///     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
///         let task = task.index() as u64;
///         let base = if self.0 == 0 { 10 + task } else { 20 - task };
///         base.checked_sub(2 * path.len() as u64)
///     }
/// }
///
/// let agents = (0..2).map(|id| Agent::new(AgentId(id), 2, 4, 2, Skewed(id as u64))).collect();
/// let network = Network { seed: 1, max_delay: 3, ..Network::default() };
/// let mut simulation = Simulation::new(agents, network);
///
/// simulation.run_until_settled(200).expect("falls quiet in agreement");
/// assert!(simulation.assignment().is_conflict_free());
/// ```
#[derive(Clone, Debug)]
pub struct Agent<V> {
    /// This agent's identifier.
    id: AgentId,

    /// How this agent values tasks.
    valuation: V,

    /// The most tasks this agent holds at once.
    capacity: usize,

    /// The tasks this agent holds, with the bid it placed on each.
    bundle: Bundle<TaskId>,

    /// The latest bid of each bidder on each task, as its gain, indexed by
    /// `bidder * tasks + task`; `None` when the bidder has none, or dropped it.
    gains: Vec<Option<u64>>,

    /// The latest time this agent has seen of each bidder on each task, in half ticks,
    /// indexed by `bidder * tasks + task`; zero before any.
    stamps: Vec<u64>,

    /// The tasks and gains of this agent's bundle as last announced.
    announced: Vec<(TaskId, u64)>,

    /// What this agent broadcasts at the end of its next round.
    queue: Vec<Entry>,

    /// Every task identifier, in order: the candidates of the bundle phase.
    tasks: Vec<TaskId>,
}

impl<V: Valuation<TaskId>> Agent<V> {
    /// Agent `id` of a team of `agents`, bidding on tasks `0..tasks` and holding at most
    /// `capacity` of them.
    ///
    /// # Panics
    ///
    /// Panics when `id` is not below `agents`.
    #[must_use]
    pub fn new(id: AgentId, agents: usize, tasks: usize, capacity: usize, valuation: V) -> Self {
        assert!(id.index() < agents, "the agent must be part of the team");

        Self {
            id,
            valuation,
            capacity,
            bundle: Bundle::new(),
            gains: vec![None; agents * tasks],
            stamps: vec![0; agents * tasks],
            announced: Vec::new(),
            queue: Vec::new(),
            tasks: (0..tasks).map(TaskId).collect(),
        }
    }

    /// The bundle phase at tick `now`: adds the most valuable tasks this agent can win, then
    /// queues a drop bid for every task it no longer holds and a bid for every new one.
    pub fn build_bundle(&mut self, now: u64) {
        let id = self.id;
        let best: Vec<Option<Bid>> = self.tasks.iter().map(|&task| self.best(task)).collect();
        self.bundle
            .extend_greedily(&self.valuation, &self.tasks, self.capacity, |task, gain| {
                wins(id, best[task.index()], gain)
            });

        self.announce_changes(now);
    }

    /// Whether the bundle phase would add a task now.
    #[must_use]
    pub fn can_bid(&self) -> bool {
        let id = self.id;

        self.bundle.len() < self.capacity
            && self
                .bundle
                .best_candidate(&self.valuation, &self.tasks, |task, gain| {
                    wins(id, self.best(*task), gain)
                })
                .is_some()
    }

    /// The consensus phase for one packet: merges its drop bids, then its bids, queuing
    /// what is new and answering bids it knows better ones for, then releases the first
    /// task in the bundle this agent was outbid on, and every task added after it.
    ///
    /// Packets from this agent itself are ignored, as are entries about unknown tasks or
    /// bidders outside the team.
    pub fn receive(&mut self, packet: &Packet) {
        if packet.sender == self.id {
            return;
        }

        for entry in &packet.entries {
            if let Entry::Drop { task, bidder, time } = *entry {
                self.merge_drop(task, bidder, time);
            }
        }

        for entry in &packet.entries {
            if let Entry::Bid { task, bid, time } = *entry {
                self.merge_bid(task, bid, time);
            }
        }

        self.release_outbid();
    }

    /// Takes a drop bid of `bidder` on `task`, made at `time`, when it is new.
    fn merge_drop(&mut self, task: TaskId, bidder: AgentId, time: u64) {
        let Some(stamp) = self.stamp_mut(bidder, task) else {
            return;
        };
        if *stamp >= time {
            return;
        }

        *stamp = time;
        self.queue.push(Entry::Drop { task, bidder, time });
        self.set_gain(bidder, task, None);
        self.queue_best(task);
    }

    /// Queues the best bid this agent knows for `task`, if any, with the time it was made:
    /// after a drop, a runner-up may have taken over that neighbors never heard of.
    fn queue_best(&mut self, task: TaskId) {
        if let Some(best) = self.best(task) {
            let time = self.stamp(best.bidder, task);
            self.queue.push(Entry::Bid {
                task,
                bid: best,
                time,
            });
        }
    }

    /// Takes `bid` on `task`, made at `time`, when it is new: keeps it, and passes it on
    /// when it is now the best known bid, or answers with the best known bid otherwise.
    fn merge_bid(&mut self, task: TaskId, bid: Bid, time: u64) {
        let Some(stamp) = self.stamp_mut(bid.bidder, task) else {
            return;
        };
        if *stamp >= time {
            return;
        }

        *stamp = time;
        self.set_gain(bid.bidder, task, Some(bid.gain));
        let best = self.best(task).expect("the bid was just kept");
        let time = self.stamp(best.bidder, task);
        self.queue.push(Entry::Bid {
            task,
            bid: best,
            time,
        });
    }

    /// Drops the first task in the bundle this agent no longer wins, and every task added
    /// after it, forgetting its own bids on them; the next bundle phase announces the drops.
    fn release_outbid(&mut self) {
        let id = self.id;
        let Some(lost) = self
            .bundle
            .tasks()
            .position(|&task| self.best(task).is_none_or(|best| best.bidder != id))
        else {
            return;
        };

        for task in self.bundle.release_from(lost) {
            self.set_gain(id, task, None);
        }
    }

    /// Queues a drop bid at `now` for every announced task the bundle no longer holds with
    /// the same gain, and a bid for every task it holds anew, then records the bundle as
    /// announced. Every task held counts as won by this agent's own bid, including one
    /// released and taken back with the same gain, which is not announced again.
    fn announce_changes(&mut self, now: u64) {
        let id = self.id;
        let current: Vec<(TaskId, u64)> = self
            .bundle
            .entries()
            .map(|(&task, gain)| (task, gain))
            .collect();

        let announced = core::mem::take(&mut self.announced);
        for &(task, gain) in &announced {
            if !current.contains(&(task, gain)) {
                let time = 2 * now + 1;
                self.set_stamp(id, task, time);
                if !current.iter().any(|&(held, _)| held == task) {
                    self.set_gain(id, task, None);
                    self.queue_best(task);
                }
                self.queue.push(Entry::Drop {
                    task,
                    bidder: id,
                    time,
                });
            }
        }

        for &(task, gain) in &current {
            let bid = Bid { bidder: id, gain };
            self.set_gain(id, task, Some(gain));
            if !announced.contains(&(task, gain)) {
                let time = 2 * now + 2;
                self.set_stamp(id, task, time);
                self.queue.push(Entry::Bid { task, bid, time });
            }
        }

        self.announced = current;
    }

    /// Records `gain` as `bidder`'s latest bid on `task`, or `None` for none.
    fn set_gain(&mut self, bidder: AgentId, task: TaskId, gain: Option<u64>) {
        let tasks = self.tasks.len();
        self.gains[bidder.index() * tasks + task.index()] = gain;
    }

    /// The latest time this agent has seen of `bidder` on `task`.
    fn stamp(&self, bidder: AgentId, task: TaskId) -> u64 {
        self.stamps[bidder.index() * self.tasks.len() + task.index()]
    }

    /// Records `time` as the latest time seen of `bidder` on `task`.
    fn set_stamp(&mut self, bidder: AgentId, task: TaskId, time: u64) {
        let tasks = self.tasks.len();
        self.stamps[bidder.index() * tasks + task.index()] = time;
    }

    /// The latest time seen of `bidder` on `task`, to update; `None` for an unknown task or
    /// a bidder outside the team.
    fn stamp_mut(&mut self, bidder: AgentId, task: TaskId) -> Option<&mut u64> {
        let tasks = self.tasks.len();
        if task.index() >= tasks {
            return None;
        }

        self.stamps.get_mut(bidder.index() * tasks + task.index())
    }

    /// The packet to broadcast now, if anything is queued; empties the queue.
    pub fn packet(&mut self) -> Option<Packet> {
        if self.queue.is_empty() {
            return None;
        }

        Some(Packet {
            sender: self.id,
            entries: core::mem::take(&mut self.queue),
        })
    }
}

impl<V> Agent<V> {
    /// This agent's identifier.
    #[must_use]
    pub const fn id(&self) -> AgentId {
        self.id
    }

    /// This agent's valuation.
    #[must_use]
    pub const fn valuation(&self) -> &V {
        &self.valuation
    }

    /// The tasks this agent holds, with the bid it placed on each.
    #[must_use]
    pub const fn bundle(&self) -> &Bundle<TaskId> {
        &self.bundle
    }

    /// The best bid this agent knows for `task`: the best of the latest bids of every
    /// bidder.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    #[must_use]
    pub fn best(&self, task: TaskId) -> Option<Bid> {
        let tasks = self.tasks.len();
        assert!(task.index() < tasks, "unknown task");

        self.gains
            .iter()
            .skip(task.index())
            .step_by(tasks)
            .enumerate()
            .filter_map(|(bidder, gain)| {
                gain.map(|gain| Bid {
                    bidder: AgentId(bidder),
                    gain,
                })
            })
            .reduce(|best, bid| if bid.outbids(best) { bid } else { best })
    }

    /// Whether this agent's bundle holds exactly the tasks whose best known bid is its own.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        let own = |task: TaskId| self.best(task).is_some_and(|best| best.bidder == self.id);
        let won = self.tasks.iter().filter(|&&task| own(task)).count();

        won == self.bundle.len() && self.bundle.tasks().all(|&task| own(task))
    }
}

/// Whether agent `id`, bidding `gain`, beats `best`: a positive bid that outbids it, or any
/// positive bid when nobody bids.
fn wins(id: AgentId, best: Option<Bid>, gain: u64) -> bool {
    gain > 0 && best.is_none_or(|best| Bid { bidder: id, gain }.outbids(best))
}

impl<V: Valuation<TaskId>> Participant for Agent<V> {
    type Message = Packet;

    fn id(&self) -> AgentId {
        self.id
    }

    fn bundle(&self) -> &Bundle<TaskId> {
        &self.bundle
    }

    /// Runs the bundle phase, then broadcasts everything queued, if anything.
    fn act(&mut self, now: u64, outbox: &mut Vec<Packet>) {
        self.build_bundle(now);
        outbox.extend(self.packet());
    }

    /// Merges `packet`; what it calls for goes out at the end of the next round.
    fn receive(&mut self, _now: u64, packet: &Packet, _outbox: &mut Vec<Packet>) {
        Agent::receive(self, packet);
    }

    /// The network is quiet, nothing is queued, every agent knows the same best bid for
    /// every task, every bundle matches, and no bundle phase would add a task.
    fn is_settled(team: &[Self], in_flight: &[&Packet]) -> bool {
        let Some(first) = team.first() else {
            return true;
        };

        in_flight.is_empty()
            && team.iter().all(|agent| agent.queue.is_empty())
            && team.iter().all(|agent| {
                agent
                    .tasks
                    .iter()
                    .all(|&task| agent.best(task) == first.best(task))
            })
            && team.iter().all(Agent::is_consistent)
            && !team.iter().any(Agent::can_bid)
    }
}

/// Unit tests for [`Agent`].
#[cfg(test)]
mod tests;
