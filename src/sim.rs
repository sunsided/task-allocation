//! A deterministic simulated network for running teams of agents: seeded delays, losses,
//! topologies and partitions.
//!
//! A [`Simulation`] runs any [`Participant`], such as a [`cbba::Agent`]. Each
//! [`Simulation::step`] is one tick:
//!
//! ```mermaid
//! flowchart LR
//!     deliver[deliver due messages] --> round[every agent's round]
//!     round --> broadcast[broadcast every outbox]
//!     broadcast --> deliver
//! ```
//!
//! Agents may also answer a delivered message right away; those answers go out with the
//! tick's broadcasts. A broadcast reaches every agent the sender has a link to in the
//! [`Topology`] and is not cut off from by an active [`Partition`]; others hear of it only
//! when it is relayed. Each copy is lost with probability [`Network::loss_per_mille`] /
//! 1000, and otherwise arrives after 1 to `1 + max_delay` ticks, so messages overtake each
//! other. Agents process the messages that arrive in a tick in arrival order. Everything
//! random comes from [`Network::seed`]: the same seed gives the same run.
//!
//! Two optional models make the network harsher: [`Network::collisions`] lets senders
//! that cannot hear each other destroy each other's copies, and [`Network::clusters`]
//! lets groups of agents exchange messages with each other only every few ticks. Both are
//! off by default and draw no randomness then, so seeded runs stay the same.
//!
//! [`Stats`] counts what the run cost: copies sent, delivered and lost, the bytes
//! broadcast, and how many delivered copies changed their receiver.
//!
//! When a team is *settled* is up to the protocol ([`Participant::is_settled`]). For CBBA,
//! every agent believes in the same winners, every agent's bundle holds exactly the tasks it
//! wins, no bundle phase would change anything, and every message still in flight carries
//! those same winners. Nothing can change after that.

mod rng;

use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::assignment::Assignment;
use crate::bundle::Bundle;
use crate::cbba;
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;
use rng::SplitMix64;

/// How the simulated network behaves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Network {
    /// Seeds every random choice: delays and losses.
    pub seed: u64,

    /// The most extra ticks a message may take, on top of the one tick every message takes.
    pub max_delay: u64,

    /// The chance, per mille, that one copy of a broadcast is lost. 0 is lossless; 1000 or
    /// more loses everything.
    pub loss_per_mille: u16,

    /// Which agents can reach each other at all.
    pub topology: Topology,

    /// Intervals in which groups of agents cannot reach each other.
    pub partitions: Vec<Partition>,

    /// Whether broadcasts from hidden nodes collide.
    ///
    /// Each tick, every sender waits a random backoff of one of [`BACKOFF_SLOTS`] slots
    /// before transmitting, as 802.11 radios do. Two senders that cannot hear each other
    /// and pick the same slot lose both their copies to every agent that hears them both.
    /// Senders that hear each other take turns, as radios sensing the channel do, so on a
    /// [`Topology::Complete`] network nothing collides. This follows the hidden-node
    /// picture in Raja, Habibi and How (IEEE Access 10, 2022, p. 19756), with their
    /// contention window of 16.
    pub collisions: bool,

    /// Clusters that talk among themselves every tick, but with each other only every
    /// few ticks; `None` for one rate everywhere.
    pub clusters: Option<Clusters>,
}

/// The backoff slots a sender picks from each tick when [`Network::collisions`] is on.
pub const BACKOFF_SLOTS: u64 = 16;

/// Groups of agents that exchange messages at different rates: inside a group as usual,
/// between groups only every `period` ticks.
///
/// A copy between groups arrives at the first multiple of `period` at or after the tick it
/// would otherwise arrive at. This is the multirate idea of hierarchical co-simulation
/// (Hafner, *Cooperative and Multirate Simulation Analysis*, TU Wien, 2021): tightly
/// coupled parts synchronize often, the whole less often.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clusters {
    /// The group of each agent, indexed by agent. Agents missing from the list form a group
    /// of their own each.
    pub groups: Vec<usize>,

    /// Ticks between two exchanges across groups; positive. One changes nothing.
    pub period: u64,
}

/// Whether `a` and `b` are in the same group of `groups`, where agents missing from the
/// list form a group of their own each.
fn same_group(groups: &[usize], a: AgentId, b: AgentId) -> bool {
    groups
        .get(a.index())
        .is_some_and(|group| groups.get(b.index()) == Some(group))
}

/// Which pairs of agents can exchange messages directly. Agents without a direct link only
/// hear of each other through relays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Topology {
    /// Every agent reaches every other agent.
    #[default]
    Complete,

    /// Only the listed pairs reach each other, in both directions.
    Links(Vec<(AgentId, AgentId)>),
}

impl Topology {
    /// Whether `a` and `b` can exchange messages directly.
    #[must_use]
    pub fn connects(&self, a: AgentId, b: AgentId) -> bool {
        match self {
            Self::Complete => true,
            Self::Links(links) => links.iter().any(|&link| link == (a, b) || link == (b, a)),
        }
    }
}

/// A time interval in which agents in different groups cannot reach each other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partition {
    /// The first tick of the partition.
    pub from: u64,

    /// The tick at which the partition heals; it is active for ticks `from..until`.
    pub until: u64,

    /// The group of each agent, indexed by agent. Agents missing from the list form a group
    /// of their own each.
    pub groups: Vec<usize>,
}

impl Partition {
    /// Whether the partition separates agents `a` and `b` at `tick`.
    #[must_use]
    pub fn separates(&self, tick: u64, a: AgentId, b: AgentId) -> bool {
        let active = (self.from..self.until).contains(&tick);

        active && !same_group(&self.groups, a, b)
    }
}

/// Message counts over a run.
///
/// Two ratios compare protocols beyond raw counts:
///
/// - **Value of a message:** `useful / delivered`, the share of delivered copies that
///   changed the receiver's winners, bids or bundle. Raja, Habibi and How ("Communication-
///   Aware Consensus-Based Decentralized Task Allocation", IEEE Access 10, 2022, Eq. 3)
///   average the number of changed winning-bid entries per neighbor instead; here a copy
///   counts once, however many entries it changed.
/// - **Bytes per completed task:** `bytes` divided by the number of tasks the final
///   [assignment](Simulation::assignment) holds, as in Zhao et al. ("AC-DMTA", Drones 10,
///   2026, Sec. 5.4). Sizes are nominal; see [`Participant::message_bytes`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Copies of broadcasts put on the network, one per reachable receiver.
    pub sent: u64,

    /// Copies that arrived.
    pub delivered: u64,

    /// Copies the network lost.
    pub lost: u64,

    /// Copies that could not be sent because the topology has no link to the receiver, or a
    /// partition cut it off.
    pub blocked: u64,

    /// Delivered copies that changed the receiver's winners, bids or bundle.
    pub useful: u64,

    /// Bytes broadcast, by [`Participant::message_bytes`], counted once per broadcast that
    /// reached at least one receiver: on a shared radio channel, one transmission reaches
    /// every neighbor.
    pub bytes: u64,

    /// Copies lost to [collisions](Network::collisions) between hidden senders; also
    /// counted in `sent`, not in `lost`.
    pub collided: u64,
}

/// An agent that a [`Simulation`] can run: it works in rounds, broadcasts messages, and
/// handles the messages that reach it.
///
/// [`cbba::Agent`] implements it; so can other consensus protocols.
pub trait Participant: Sized {
    /// What the agent broadcasts.
    type Message;

    /// The agent's identifier.
    fn id(&self) -> AgentId;

    /// The tasks the agent holds now.
    fn bundle(&self) -> &Bundle<TaskId>;

    /// One round of the agent's own work at tick `now`, such as its bundle phase; pushes
    /// what it broadcasts onto `outbox`.
    fn act(&mut self, now: u64, outbox: &mut Vec<Self::Message>);

    /// Handles `message`, arriving at tick `now`; pushes what the agent broadcasts in
    /// response onto `outbox`. Returns whether the agent's winners, bids or bundle changed,
    /// for [`Stats::useful`].
    fn receive(
        &mut self,
        now: u64,
        message: &Self::Message,
        outbox: &mut Vec<Self::Message>,
    ) -> bool;

    /// The size of `message` on the wire, in bytes, for [`Stats::bytes`].
    ///
    /// The protocols in this crate use a nominal encoding without compression: 4 bytes per
    /// agent or task identifier (an absent winner takes a reserved value), 8 per bid,
    /// timestamp or clock, 1 per tag, and a 4-byte length before every list.
    fn message_bytes(message: &Self::Message) -> u64;

    /// Whether `team` has settled, with `in_flight` still on its way: the agents agree on a
    /// conflict-free assignment that nothing in flight and no further round can change.
    fn is_settled(team: &[Self], in_flight: &[&Self::Message]) -> bool;
}

impl<V: Valuation<TaskId>> Participant for cbba::Agent<V> {
    type Message = cbba::Message;

    fn id(&self) -> AgentId {
        self.id()
    }

    fn bundle(&self) -> &Bundle<TaskId> {
        self.bundle()
    }

    /// Runs the bundle phase and broadcasts the agent's full state when its
    /// [`Cadence`](cbba::Cadence) calls for it; by default, every round.
    fn act(&mut self, _now: u64, outbox: &mut Vec<cbba::Message>) {
        if self.has_left() {
            return;
        }

        self.build_bundle();
        if self.broadcast_due() {
            outbox.push(self.message());
        }
    }

    /// Runs the consensus phase for `message`; CBBA answers in the next round, not now.
    fn receive(
        &mut self,
        _now: u64,
        message: &cbba::Message,
        _outbox: &mut Vec<cbba::Message>,
    ) -> bool {
        cbba::Agent::receive(self, message)
    }

    /// The sender, every task's winner and bid, and every agent's clock.
    fn message_bytes(message: &cbba::Message) -> u64 {
        message.bytes()
    }

    /// The team agrees on every winner, every bundle matches, no agent's bundle phase would
    /// change anything, and every message in flight carries the agreed winners. A team
    /// that has not bid yet is not settled.
    ///
    /// Agents that left the team, and messages they sent, are disregarded.
    fn is_settled(team: &[Self], in_flight: &[&cbba::Message]) -> bool {
        let mut active = team.iter().filter(|agent| !agent.has_left());
        let Some(first) = active.next() else {
            return true;
        };

        let active = || team.iter().filter(|agent| !agent.has_left());

        active().all(cbba::Agent::is_consistent)
            && !active().any(cbba::Agent::can_bid)
            && active().all(|agent| agent.claims() == first.claims())
            && in_flight
                .iter()
                .filter(|message| !first.has_departed(message.sender()))
                .all(|message| message.claims() == first.claims())
    }
}

/// Message copies on their way.
type Copies<M> = Vec<InFlight<M>>;

/// A message copy on its way to one agent.
#[derive(Clone, Debug)]
struct InFlight<M> {
    /// The tick at which the copy arrives.
    arrives: u64,

    /// Sending order, to deliver copies arriving in the same tick deterministically.
    order: u64,

    /// The receiver.
    to: AgentId,

    /// The message, shared by all copies of one broadcast.
    message: Rc<M>,
}

/// A team of agents on a simulated network.
///
/// # Examples
///
/// ```
/// use task_allocation::{AgentId, TaskId, Valuation, cbba::Agent, sim::{Network, Simulation}};
///
/// /// Values task `t` at `10 + t`, minus 3 for every task already held.
/// struct Worker;
///
/// impl Valuation<TaskId> for Worker {
///     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
///         (10 + task.index() as u64).checked_sub(3 * path.len() as u64)
///     }
/// }
///
/// let agents = (0..3).map(|id| Agent::new(AgentId(id), 3, 5, 2, Worker)).collect();
/// let network = Network { seed: 7, max_delay: 3, loss_per_mille: 200, ..Network::default() };
/// let mut simulation = Simulation::new(agents, network);
///
/// let ticks = simulation.run_until_settled(500).expect("settles");
/// assert!(simulation.assignment().is_conflict_free());
/// assert!(ticks <= 500);
/// ```
#[derive(Clone, Debug)]
pub struct Simulation<A: Participant> {
    /// The team, indexed by agent.
    agents: Vec<A>,

    /// The network's behavior.
    network: Network,

    /// The source of delays and losses.
    rng: SplitMix64,

    /// The current tick: how many steps have run.
    tick: u64,

    /// Copies still on their way.
    in_flight: Copies<A::Message>,

    /// Copies due this tick, kept between ticks so delivering does not allocate.
    due: Copies<A::Message>,

    /// Each agent's outbox for this tick, kept between ticks so stepping does not allocate.
    outboxes: Vec<Vec<A::Message>>,

    /// The sending order of the next copy.
    next_order: u64,

    /// Message counts so far.
    stats: Stats,
}

impl<A: Participant> Simulation<A> {
    /// A simulation of `agents` on `network`, at tick zero with nothing in flight.
    ///
    /// # Panics
    ///
    /// Panics when the agents are not identified `0..agents.len()` in order, and when the
    /// network's [`Clusters::period`] is zero.
    #[must_use]
    pub fn new(agents: Vec<A>, network: Network) -> Self {
        let in_order = agents
            .iter()
            .enumerate()
            .all(|(index, agent)| agent.id() == AgentId(index));
        assert!(in_order, "agents must be identified 0, 1, 2, ... in order");
        assert!(
            network
                .clusters
                .as_ref()
                .is_none_or(|clusters| clusters.period > 0),
            "clusters must exchange messages every positive number of ticks"
        );

        let rng = SplitMix64::new(network.seed);

        Self {
            agents,
            network,
            rng,
            tick: 0,
            in_flight: Vec::new(),
            due: Vec::new(),
            outboxes: Vec::new(),
            next_order: 0,
            stats: Stats::default(),
        }
    }

    /// Runs one tick: delivers the messages due, runs every agent's round, and broadcasts
    /// what the agents put in their outboxes.
    pub fn step(&mut self) {
        let mut outboxes = core::mem::take(&mut self.outboxes);
        outboxes.resize_with(self.agents.len(), Vec::new);
        self.deliver_due(&mut outboxes);

        for (agent, outbox) in self.agents.iter_mut().zip(&mut outboxes) {
            agent.act(self.tick, outbox);
        }

        self.broadcast_all(&mut outboxes);
        self.outboxes = outboxes;
        self.tick += 1;
    }

    /// Steps until the team is [settled](Simulation::is_settled), at most `max_ticks` times;
    /// returns the tick at which it settled, or `None` when it did not within the limit.
    pub fn run_until_settled(&mut self, max_ticks: u64) -> Option<u64> {
        let limit = self.tick.saturating_add(max_ticks);
        while self.tick < limit {
            self.step();
            if self.is_settled() {
                return Some(self.tick);
            }
        }

        None
    }

    /// Whether the team has settled: see [`Participant::is_settled`]. Nothing changes after
    /// that.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        let in_flight: Vec<&A::Message> =
            self.in_flight.iter().map(|copy| &*copy.message).collect();

        A::is_settled(&self.agents, &in_flight)
    }

    /// Delivers every copy due by the current tick, in arrival order, collecting what the
    /// receivers broadcast in response in `outboxes`.
    fn deliver_due(&mut self, outboxes: &mut [Vec<A::Message>]) {
        let tick = self.tick;
        let mut due = core::mem::take(&mut self.due);
        let mut index = 0;
        while index < self.in_flight.len() {
            if self.in_flight[index].arrives <= tick {
                due.push(self.in_flight.swap_remove(index));
            } else {
                index += 1;
            }
        }

        due.sort_unstable_by_key(|copy| (copy.arrives, copy.order));
        for copy in due.drain(..) {
            let to = copy.to.index();
            let changed = self.agents[to].receive(tick, &copy.message, &mut outboxes[to]);
            self.stats.delivered += 1;
            self.stats.useful += u64::from(changed);
        }

        self.due = due;
    }

    /// Broadcasts every message in every agent's outbox, leaving the outboxes empty.
    fn broadcast_all(&mut self, outboxes: &mut [Vec<A::Message>]) {
        let agents = self.agents.len();
        let slots = self.backoff_slots(outboxes);

        for (sender, outbox) in outboxes.iter_mut().enumerate() {
            for message in outbox.drain(..) {
                let message = Rc::new(message);
                let mut transmitted = false;
                for receiver in (0..agents).filter(|&receiver| receiver != sender) {
                    let (from, to) = (AgentId(sender), AgentId(receiver));
                    let collides = self.collides(&slots, from, to);
                    transmitted |= self.send(from, to, &message, collides);
                }

                if transmitted {
                    self.stats.bytes += A::message_bytes(&message);
                }
            }
        }
    }

    /// Each agent's backoff slot this tick, `None` when it sends nothing; empty when
    /// [collisions](Network::collisions) are off, so that no randomness is drawn.
    fn backoff_slots(&mut self, outboxes: &[Vec<A::Message>]) -> Vec<Option<u64>> {
        if !self.network.collisions {
            return Vec::new();
        }

        outboxes
            .iter()
            .map(|outbox| (!outbox.is_empty()).then(|| self.rng.below(BACKOFF_SLOTS)))
            .collect()
    }

    /// Whether the copy from `from` to `to` collides this tick: another sender in the same
    /// backoff slot reaches `to` but cannot hear `from`. Empty `slots` means collisions are
    /// off.
    fn collides(&self, slots: &[Option<u64>], from: AgentId, to: AgentId) -> bool {
        let Some(&Some(slot)) = slots.get(from.index()) else {
            return false;
        };

        slots.iter().enumerate().any(|(other, &other_slot)| {
            let other = AgentId(other);

            other_slot == Some(slot)
                && other != from
                && other != to
                && !self.is_cut_off(other, to)
                && self.is_cut_off(from, other)
        })
    }

    /// Sends one copy of `message` from `from` to `to`, unless a partition blocks it, it
    /// `collides` with a hidden sender's, or the network loses it; returns whether the copy
    /// went on the air, lost or not.
    fn send(
        &mut self,
        from: AgentId,
        to: AgentId,
        message: &Rc<A::Message>,
        collides: bool,
    ) -> bool {
        if self.is_cut_off(from, to) {
            self.stats.blocked += 1;
            return false;
        }

        self.stats.sent += 1;
        if collides {
            self.stats.collided += 1;
            return true;
        }

        if self.rng.below(1000) < u64::from(self.network.loss_per_mille) {
            self.stats.lost += 1;
            return true;
        }

        let delay = 1 + self.rng.below(self.network.max_delay.saturating_add(1));
        self.in_flight.push(InFlight {
            arrives: self.arrival(from, to, self.tick + delay),
            order: self.next_order,
            to,
            message: Rc::clone(message),
        });
        self.next_order += 1;

        true
    }

    /// When a copy from `from` to `to` that would arrive at `tick` does arrive: then, or,
    /// between [clusters](Network::clusters), at the next exchange across them.
    fn arrival(&self, from: AgentId, to: AgentId, tick: u64) -> u64 {
        match &self.network.clusters {
            Some(clusters) if !same_group(&clusters.groups, from, to) => {
                tick.div_ceil(clusters.period) * clusters.period
            }
            _ => tick,
        }
    }

    /// Whether `a` and `b` have no direct link, or an active partition separates them now.
    fn is_cut_off(&self, a: AgentId, b: AgentId) -> bool {
        !self.network.topology.connects(a, b)
            || self
                .network
                .partitions
                .iter()
                .any(|partition| partition.separates(self.tick, a, b))
    }

    /// The team, indexed by agent.
    #[must_use]
    pub fn agents(&self) -> &[A] {
        &self.agents
    }

    /// The team, to change it between steps: add a task, retire one, or let an agent leave.
    pub fn agents_mut(&mut self) -> &mut [A] {
        &mut self.agents
    }

    /// The current tick: how many steps have run.
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// Message counts so far.
    #[must_use]
    pub const fn stats(&self) -> Stats {
        self.stats
    }

    /// Every agent's bundle, as one assignment. It is conflict-free once the team is
    /// [settled](Simulation::is_settled); before, agents may still hold the same task.
    #[must_use]
    pub fn assignment(&self) -> Assignment {
        Assignment::from_bundles(
            self.agents
                .iter()
                .map(|agent| agent.bundle().clone())
                .collect(),
        )
    }
}

/// Unit tests for the simulated network.
#[cfg(test)]
mod tests;
