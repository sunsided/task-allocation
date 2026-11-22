//! A deterministic simulated network for running CBBA teams: seeded delays, losses and
//! partitions.
//!
//! Each [`Simulation::step`] is one tick:
//!
//! ```mermaid
//! flowchart LR
//!     deliver[deliver due messages] --> bundle[bundle phase, every agent]
//!     bundle --> broadcast[broadcast to reachable agents]
//!     broadcast --> deliver
//! ```
//!
//! A broadcast reaches every other agent not cut off by an active [`Partition`]. Each copy is
//! lost with probability [`Network::loss_per_mille`] / 1000, and otherwise arrives after
//! 1 to `1 + max_delay` ticks, so messages overtake each other. Agents process the messages
//! that arrive in a tick in arrival order. Everything random comes from [`Network::seed`]: the
//! same seed gives the same run.
//!
//! A team is *settled* when every agent believes in the same winners, every agent's bundle
//! holds exactly the tasks it wins, and every message still in flight carries those same
//! winners. Nothing can change after that.

mod rng;

use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::assignment::Assignment;
use crate::cbba::{Agent, Message};
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

    /// Intervals in which groups of agents cannot reach each other.
    pub partitions: Vec<Partition>,
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

        active
            && self
                .groups
                .get(a.index())
                .is_none_or(|group| self.groups.get(b.index()) != Some(group))
    }
}

/// Message counts over a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Copies of broadcasts put on the network, one per reachable receiver.
    pub sent: u64,

    /// Copies that arrived.
    pub delivered: u64,

    /// Copies the network lost.
    pub lost: u64,

    /// Copies that could not be sent because a partition cut the receiver off.
    pub blocked: u64,
}

/// A message copy on its way to one agent.
#[derive(Clone, Debug)]
struct InFlight {
    /// The tick at which the copy arrives.
    arrives: u64,

    /// Sending order, to deliver copies arriving in the same tick deterministically.
    order: u64,

    /// The receiver.
    to: AgentId,

    /// The message, shared by all copies of one broadcast.
    message: Rc<Message>,
}

/// A team of CBBA agents on a simulated network.
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
pub struct Simulation<V> {
    /// The team, indexed by agent.
    agents: Vec<Agent<V>>,

    /// The network's behavior.
    network: Network,

    /// The source of delays and losses.
    rng: SplitMix64,

    /// The current tick: how many steps have run.
    tick: u64,

    /// Copies still on their way.
    in_flight: Vec<InFlight>,

    /// The sending order of the next copy.
    next_order: u64,

    /// Message counts so far.
    stats: Stats,
}

impl<V: Valuation<TaskId>> Simulation<V> {
    /// A simulation of `agents` on `network`, at tick zero with nothing in flight.
    ///
    /// # Panics
    ///
    /// Panics when the agents are not identified `0..agents.len()` in order.
    #[must_use]
    pub fn new(agents: Vec<Agent<V>>, network: Network) -> Self {
        let in_order = agents
            .iter()
            .enumerate()
            .all(|(index, agent)| agent.id() == AgentId(index));
        assert!(in_order, "agents must be identified 0, 1, 2, ... in order");

        let rng = SplitMix64::new(network.seed);

        Self {
            agents,
            network,
            rng,
            tick: 0,
            in_flight: Vec::new(),
            next_order: 0,
            stats: Stats::default(),
        }
    }

    /// Runs one tick: delivers the messages due, runs every agent's bundle phase, and
    /// broadcasts every agent's state.
    pub fn step(&mut self) {
        self.deliver_due();

        for agent in &mut self.agents {
            agent.build_bundle();
        }

        self.broadcast_all();
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

    /// Delivers every copy due by the current tick, in arrival order.
    fn deliver_due(&mut self) {
        let tick = self.tick;
        let (mut due, waiting): (Vec<InFlight>, Vec<InFlight>) = self
            .in_flight
            .drain(..)
            .partition(|copy| copy.arrives <= tick);
        self.in_flight = waiting;
        due.sort_unstable_by_key(|copy| (copy.arrives, copy.order));

        for copy in due {
            self.agents[copy.to.index()].receive(&copy.message);
            self.stats.delivered += 1;
        }
    }

    /// Puts every agent's message on the network.
    fn broadcast_all(&mut self) {
        for sender in 0..self.agents.len() {
            let message = Rc::new(self.agents[sender].message());
            for receiver in 0..self.agents.len() {
                if receiver != sender {
                    self.send(AgentId(sender), AgentId(receiver), &message);
                }
            }
        }
    }

    /// Sends one copy of `message` from `from` to `to`, unless a partition blocks it or the
    /// network loses it.
    fn send(&mut self, from: AgentId, to: AgentId, message: &Rc<Message>) {
        if self.is_cut_off(from, to) {
            self.stats.blocked += 1;
            return;
        }

        self.stats.sent += 1;
        if self.rng.below(1000) < u64::from(self.network.loss_per_mille) {
            self.stats.lost += 1;
            return;
        }

        let delay = 1 + self.rng.below(self.network.max_delay.saturating_add(1));
        self.in_flight.push(InFlight {
            arrives: self.tick + delay,
            order: self.next_order,
            to,
            message: Rc::clone(message),
        });
        self.next_order += 1;
    }

    /// Whether an active partition separates `a` and `b` now.
    fn is_cut_off(&self, a: AgentId, b: AgentId) -> bool {
        self.network
            .partitions
            .iter()
            .any(|partition| partition.separates(self.tick, a, b))
    }
}

impl<V> Simulation<V> {
    /// The team, indexed by agent.
    #[must_use]
    pub fn agents(&self) -> &[Agent<V>] {
        &self.agents
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

    /// Whether the team agrees on every winner, every bundle matches, and every message in
    /// flight carries the agreed winners; nothing changes after that.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        let Some(first) = self.agents.first() else {
            return true;
        };

        self.agents.iter().all(Agent::is_consistent)
            && self
                .agents
                .iter()
                .all(|agent| agent.claims() == first.claims())
            && self
                .in_flight
                .iter()
                .all(|copy| copy.message.claims() == first.claims())
    }
}

/// Unit tests for the simulated network.
#[cfg(test)]
mod tests;
