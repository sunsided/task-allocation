//! Exhaustive exploration of message schedules on tiny teams.
//!
//! A seeded [`Simulation`](task_allocation::sim::Simulation) covers only the schedules its
//! seeds happen to produce. The explorer instead enumerates every order of events, up to
//! bounds: an agent's round ([`Participant::act`]), the delivery of any message copy in
//! flight, and, within a loss budget, the loss of one. Delays and reordering are copies
//! not delivered yet. States reached more than once are explored once, keyed by a hash of
//! their full [`Debug`] rendering, so the key covers every field that affects behavior.
//!
//! The seam mirrors the simulator's: what an agent produces while receiving waits in its
//! outbox, and goes out, together with what its next round adds, when that round ends.

mod acbba;
mod cbba;
mod ccbba;

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use task_allocation::AgentId;
use task_allocation::sim::{Participant, Topology};

/// How far the explorer goes.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    /// The most rounds each agent runs.
    pub acts: u32,

    /// The most message copies lost in one schedule.
    pub losses: u32,
}

/// One point of a schedule: the team, what each agent will broadcast next, and the copies
/// on their way.
#[derive(Debug)]
pub struct State<A: Participant> {
    /// The team, indexed by agent.
    pub agents: Vec<A>,

    /// For each agent, what it broadcasts at the end of its next round.
    pub outboxes: Vec<Vec<A::Message>>,

    /// Copies on their way, with their receiver.
    pub in_flight: Vec<(AgentId, Rc<A::Message>)>,

    /// Rounds each agent has run.
    acts: Vec<u32>,

    /// Copies lost so far.
    losses: u32,

    /// Events so far, passed to the agents as the current time.
    now: u64,

    /// The events that led here, in order; not part of the state's identity.
    pub trace: Vec<String>,
}

/// What exploring found.
#[derive(Clone, Copy, Debug, Default)]
pub struct Report {
    /// Distinct states visited.
    pub states: usize,

    /// Times a state was reached again and skipped.
    pub revisits: usize,
}

impl<A> Clone for State<A>
where
    A: Participant + Clone,
    A::Message: Clone,
{
    fn clone(&self) -> Self {
        Self {
            agents: self.agents.clone(),
            outboxes: self.outboxes.clone(),
            in_flight: self.in_flight.clone(),
            acts: self.acts.clone(),
            losses: self.losses,
            now: self.now,
            trace: self.trace.clone(),
        }
    }
}

impl<A> State<A>
where
    A: Participant + Clone + Debug,
    A::Message: Clone + Debug,
{
    /// The start of every schedule: `agents` before their first round.
    pub fn new(agents: Vec<A>) -> Self {
        let team = agents.len();

        Self {
            agents,
            outboxes: (0..team).map(|_| Vec::new()).collect(),
            in_flight: Vec::new(),
            acts: vec![0; team],
            losses: 0,
            now: 0,
            trace: Vec::new(),
        }
    }

    /// Runs agent `agent`'s round, then broadcasts its outbox over `topology`.
    pub fn act(&mut self, agent: usize, topology: &Topology) {
        let now = self.now;
        self.agents[agent].act(now, &mut self.outboxes[agent]);
        self.acts[agent] += 1;
        self.now += 1;

        let sender = AgentId(agent);
        for message in self.outboxes[agent].drain(..) {
            let message = Rc::new(message);
            for receiver in (0..self.agents.len()).map(AgentId) {
                if receiver != sender && topology.connects(sender, receiver) {
                    self.in_flight.push((receiver, Rc::clone(&message)));
                }
            }
        }
    }

    /// Delivers the copy at `index` in flight.
    pub fn deliver(&mut self, index: usize) {
        let (to, message) = self.in_flight.remove(index);
        let now = self.now;
        self.agents[to.index()].receive(now, &message, &mut self.outboxes[to.index()]);
        self.now += 1;
    }

    /// Loses the copy at `index` in flight.
    fn lose(&mut self, index: usize) {
        self.in_flight.remove(index);
        self.losses += 1;
    }

    /// A 128-bit hash of everything that affects what happens next. The time is left out,
    /// since the agents under exploration ignore it; copies in flight count as a multiset.
    /// At ten million states a 64-bit key would collide with a probability of about one in
    /// four hundred, silently skipping a state; 128 bits make that negligible, while keeping
    /// the rendered states themselves would not fit in memory.
    fn key(&self) -> u128 {
        let mut copies: Vec<String> = self
            .in_flight
            .iter()
            .map(|copy| format!("{copy:?}"))
            .collect();
        copies.sort_unstable();
        let rendered = format!(
            "{:?}|{:?}|{copies:?}|{:?}|{}",
            self.agents, self.outboxes, self.acts, self.losses
        );

        let half = |salt: u8| {
            let mut hasher = DefaultHasher::new();
            salt.hash(&mut hasher);
            rendered.hash(&mut hasher);
            hasher.finish()
        };

        (u128::from(half(0)) << 64) | u128::from(half(1))
    }

    /// Every state one event after this one, within `bounds`.
    fn successors(&self, topology: &Topology, bounds: Bounds) -> Vec<Self> {
        let mut next = Vec::new();
        for agent in 0..self.agents.len() {
            if self.acts[agent] < bounds.acts {
                let mut state = self.clone();
                state.act(agent, topology);
                state.trace.push(format!("agent {agent} runs a round"));
                next.push(state);
            }
        }

        for index in 0..self.in_flight.len() {
            let (to, message) = &self.in_flight[index];
            let copy = format!("{message:?} to agent {}", to.index());

            let mut delivered = self.clone();
            delivered.deliver(index);
            delivered.trace.push(format!("deliver {copy}"));
            next.push(delivered);

            if self.losses < bounds.losses {
                let mut lost = self.clone();
                lost.lose(index);
                lost.trace.push(format!("lose {copy}"));
                next.push(lost);
            }
        }

        next
    }

    /// Lossless synchronous rounds from here: every copy in flight arrives, then every
    /// agent runs its round and its broadcast arrives before the next. Returns the rounds
    /// it took until the team settled, or `None` when it did not within `limit`.
    pub fn close(&mut self, topology: &Topology, limit: u32) -> Option<u32> {
        for round in 0..=limit {
            while !self.in_flight.is_empty() {
                self.deliver(0);
            }

            if A::is_settled(&self.agents, &[]) {
                return Some(round);
            }

            for agent in 0..self.agents.len() {
                self.act(agent, topology);
            }
        }

        None
    }
}

/// Visits every state reachable from `start` over `topology` within `bounds`, depth first,
/// calling `visit` once per distinct state; stops at the first error `visit` returns.
pub fn explore<A>(
    start: State<A>,
    topology: &Topology,
    bounds: Bounds,
    mut visit: impl FnMut(&State<A>) -> Result<(), String>,
) -> Result<Report, String>
where
    A: Participant + Clone + Debug,
    A::Message: Clone + Debug,
{
    let mut seen = HashSet::new();
    let mut report = Report::default();
    let mut stack = vec![start];

    while let Some(state) = stack.pop() {
        if !seen.insert(state.key()) {
            report.revisits += 1;
            continue;
        }

        report.states += 1;
        visit(&state)?;
        stack.extend(state.successors(topology, bounds));
    }

    Ok(report)
}
