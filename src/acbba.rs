//! Asynchronous CBBA (ACBBA): agents build bundles and resolve conflicts on their own
//! schedule, exchanging one task at a time, and only pass on what is new.
//!
//! **Experimental: ACBBA does not settle reliably.** None of the published decision
//! tables settles every run, even on a network that loses nothing. With the default
//! 2011 table, about 1 in 1,000 simulated runs on sparse networks never goes quiet, as a
//! bid an agent has released outlives the release. For dependable results, use
//! [`cbba`](crate::cbba), which tolerates delayed, lost and reordered messages too.
//!
//! Each agent keeps, for every task, the winner it believes in, the winning bid, and the time
//! that bid was made. Its bundle phase bids as in [CBBA](crate::cbba) and announces each new
//! bid. Every arriving [`Message`] is resolved with a deconfliction table by Johnson, Ponda,
//! Choi and How, which also decides what to rebroadcast: the receiver's own entry, the
//! sender's, an empty bid, or nothing. When the network has nothing new left to say, it
//! goes quiet. The authors published three revisions of the table, which differ; an agent
//! follows the one [`Rules`] names, by default the 2011 one from "Asynchronous
//! Decentralized Task Allocation for Dynamic Environments" (AIAA Infotech@Aerospace 2011).
//!
//! Bid times come from a clock all agents share, passed in as `now`; timestamps of different
//! agents are compared directly. [`sim`](crate::sim) uses its tick.
//!
//! Choices this implementation makes where the papers are silent:
//!
//! - The bundle phase announces each task it adds, as the agent's own entry.
//! - An agent outbid on a task releases it and every task added after it, as in CBBA, and
//!   announces the later ones, which it held itself, as empty bids at the current time.
//!   Otherwise the others would keep believing it still wins them.
//! - Rebroadcasts are batched: of everything an agent decides to send about one task in a
//!   round, only the last message goes out. Answering every stale message one by one makes
//!   stale information multiply: each answer reaches every neighbor and draws answers of its
//!   own, while the winner keeps re-stamping its bid, so the network never goes quiet.
//! - Messages are only sent in response to something new. A lost message is not repeated,
//!   so on a lossy network the team can go quiet without agreeing. Even without losses,
//!   a released bid that is newer than the true winner's can survive on a distant agent.
//!   [`Agent::announce_every`] makes each agent re-announce its whole view at a fixed
//!   interval, confirming its own winning bids with the current time.

mod table;

pub use table::{Claim, Rules};

use alloc::vec;
use alloc::vec::Vec;

use crate::bundle::Bundle;
use crate::id::{AgentId, TaskId};
use crate::quiet::Quiet;
use crate::sim::Participant;
use crate::valuation::Valuation;
use table::{Broadcast, Change, Exchange};

/// One agent's entry for one task, broadcast to its neighbors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Message {
    /// The agent that broadcast it: the bidder, or an agent passing it on.
    pub sender: AgentId,

    /// The task the entry is about.
    pub task: TaskId,

    /// The sender's entry for the task.
    pub claim: Claim,
}

/// One ACBBA agent: its valuation, its bundle, and its view of every task's winner.
///
/// # Examples
///
/// ```
/// use task_allocation::acbba::Agent;
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
/// let agents = (0..2).map(|id| Agent::new(AgentId(id), 4, 2, Skewed(id as u64))).collect();
/// let network = Network { seed: 1, max_delay: 3, ..Network::default() };
/// let mut simulation = Simulation::new(agents, network);
///
/// simulation.run_until_settled(200).expect("goes quiet in agreement");
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

    /// For each task, the winner, bid and bid time this agent believes in.
    claims: Vec<Claim>,

    /// Every task identifier, in order: the candidates of the bundle phase.
    tasks: Vec<TaskId>,

    /// Times closer than this count as equal; `ε_t` in the paper.
    epsilon: u64,

    /// Re-announce the whole view every this many ticks; `None` never does.
    announce_every: Option<u64>,

    /// The decision table this agent follows.
    rules: Rules,

    /// Whether this agent breaks ties between two other winners; see
    /// [`Agent::with_printed_ties`].
    third_party_ties: bool,

    /// How many bundle phases in a row this agent's winners and bids stayed the same; bid
    /// times are left out, since re-announcements move them without changing anything.
    quiet: Quiet<(Option<AgentId>, u64)>,
}

impl<V: Valuation<TaskId>> Agent<V> {
    /// Agent `id`, bidding on tasks `0..tasks` and holding at most `capacity` of them.
    ///
    /// Times count as equal only when identical (`epsilon` of one tick), and the agent never
    /// re-announces on its own.
    #[must_use]
    pub fn new(id: AgentId, tasks: usize, capacity: usize, valuation: V) -> Self {
        Self {
            id,
            valuation,
            capacity,
            bundle: Bundle::new(),
            claims: vec![Claim::NONE; tasks],
            tasks: (0..tasks).map(TaskId).collect(),
            epsilon: 1,
            announce_every: None,
            rules: Rules::default(),
            third_party_ties: true,
            quiet: Quiet::new(),
        }
    }

    /// This agent, following the decision table `rules`.
    #[must_use]
    pub const fn with_rules(mut self, rules: Rules) -> Self {
        self.rules = rules;
        self
    }

    /// This agent, leaving ties between two other winners as the published tables do, in
    /// rows 9 and 27, instead of giving them to the smaller winner. With integer scores,
    /// two neighbors that disagree about such a tie can answer each other forever.
    #[must_use]
    pub const fn with_printed_ties(mut self) -> Self {
        self.third_party_ties = false;
        self
    }

    /// This agent, counting times closer than `epsilon` ticks as equal.
    #[must_use]
    pub const fn with_epsilon(mut self, epsilon: u64) -> Self {
        self.epsilon = epsilon;
        self
    }

    /// This agent, re-announcing its entry for every task every `period` ticks, with its own
    /// winning bids confirmed at the current time. This is not part of the published
    /// algorithm.
    ///
    /// It makes up for lost messages, and it lets the true winner of a task override a
    /// stale claim that looks newer: under the table's time rules, a lower but newer bid
    /// beats a higher but older one, so a bid an agent made and later released can outlive
    /// the release on a distant agent, even without losses.
    ///
    /// # Panics
    ///
    /// Panics when `period` is zero.
    #[must_use]
    pub fn announce_every(mut self, period: u64) -> Self {
        assert!(period > 0, "the announcement period must be positive");
        self.announce_every = Some(period);
        self
    }

    /// The bundle phase at time `now`: adds the most valuable tasks this agent can win until
    /// its bundle is full or nothing is left to win, and returns the announcements of the
    /// new bids.
    ///
    /// Each call counts as a round for [`quiet_rounds`](Agent::quiet_rounds).
    pub fn build_bundle(&mut self, now: u64) -> Vec<Message> {
        let announcements = self.bundle_phase(now);
        self.quiet
            .observe(self.claims.iter().map(|claim| (claim.winner, claim.bid)));

        announcements
    }

    /// The bundle phase proper, as [`build_bundle`](Agent::build_bundle) describes it.
    fn bundle_phase(&mut self, now: u64) -> Vec<Message> {
        let id = self.id;
        let claims = &self.claims;
        let before = self.bundle.len();
        self.bundle
            .extend_greedily(&self.valuation, &self.tasks, self.capacity, |task, gain| {
                wins(id, claims[task.index()], gain)
            });

        let mut announcements = Vec::new();
        for (&task, gain) in self.bundle.entries().skip(before) {
            let claim = Claim {
                winner: Some(id),
                bid: gain,
                time: now,
            };
            self.claims[task.index()] = claim;
            announcements.push(self.message(task, claim));
        }

        announcements
    }

    /// How many bundle phases in a row this agent's winners and bids stayed the same; bid
    /// times alone do not count as a change.
    #[must_use]
    pub const fn quiet_rounds(&self) -> u64 {
        self.quiet.rounds()
    }

    /// This agent's own guess that the team has settled: its winners and bids stayed the
    /// same for at least `span` bundle phases, the *separation time* of the 2011 paper
    /// (pp. 8–9). A heuristic that can be wrong both ways; see
    /// [`cbba::Agent::is_locally_settled`](crate::cbba::Agent::is_locally_settled).
    #[must_use]
    pub const fn is_locally_settled(&self, span: u64) -> bool {
        self.quiet.rounds() >= span
    }

    /// Whether the bundle phase would add a task now.
    #[must_use]
    pub fn can_bid(&self) -> bool {
        let id = self.id;
        let claims = &self.claims;

        self.bundle.len() < self.capacity
            && self
                .bundle
                .best_candidate(&self.valuation, &self.tasks, |task, gain| {
                    wins(id, claims[task.index()], gain)
                })
                .is_some()
    }

    /// Resolves `message`, arriving at time `now`, and returns what to rebroadcast.
    ///
    /// Messages from this agent itself and about unknown tasks are ignored.
    pub fn receive(&mut self, now: u64, message: &Message) -> Vec<Message> {
        let task = message.task;
        if message.sender == self.id || task.index() >= self.claims.len() {
            return Vec::new();
        }

        let exchange = Exchange {
            receiver: self.id,
            sender: message.sender,
            epsilon: self.epsilon,
            rules: self.rules,
            third_party_ties: self.third_party_ties,
        };
        let decision = exchange.decide(message.claim, self.claims[task.index()]);
        self.apply(task, decision.change, message.claim, now);

        let mut outgoing: Vec<Message> = self
            .rebroadcast(task, decision.broadcast, message.claim, now)
            .into_iter()
            .collect();
        outgoing.extend(self.release_outbid(now));

        outgoing
    }

    /// Changes this agent's entry for `task` as decided.
    fn apply(&mut self, task: TaskId, change: Change, sent: Claim, now: u64) {
        let held = &mut self.claims[task.index()];
        match change {
            Change::Update => *held = sent,
            Change::UpdateTime => held.time = now,
            Change::UpdatePast => {
                let time = if sent.time > held.time {
                    sent.time
                } else {
                    held.time + self.epsilon
                };
                *held = Claim { time, ..sent };
            }
            Change::KeepPast => {
                if held.time <= sent.time {
                    held.time = sent.time + self.epsilon;
                }
            }
            Change::Reset => {
                *held = match self.rules {
                    Rules::Gnc2010 => Claim::NONE,
                    Rules::Infotech2011 | Rules::Thesis2012 => Claim::empty_at(now),
                }
            }
            Change::Leave => {}
        }
    }

    /// The message to rebroadcast about `task`, if any.
    fn rebroadcast(
        &self,
        task: TaskId,
        broadcast: Broadcast,
        sent: Claim,
        now: u64,
    ) -> Option<Message> {
        let claim = match broadcast {
            Broadcast::Nothing => return None,
            Broadcast::Own => self.claims[task.index()],
            Broadcast::Sender => sent,
            Broadcast::EmptyNow => Claim::empty_at(now),
        };

        Some(self.message(task, claim))
    }

    /// Drops the first task in the bundle this agent no longer wins, and everything added
    /// after it; returns empty bids at `now` for the later tasks it held itself.
    fn release_outbid(&mut self, now: u64) -> Vec<Message> {
        let Some(lost) = self
            .bundle
            .tasks()
            .position(|task| self.claims[task.index()].winner != Some(self.id))
        else {
            return Vec::new();
        };

        let mut announcements = Vec::new();
        for task in self.bundle.release_from(lost).into_iter().skip(1) {
            if self.claims[task.index()].winner == Some(self.id) {
                let empty = Claim::empty_at(now);
                self.claims[task.index()] = empty;
                announcements.push(self.message(task, empty));
            }
        }

        announcements
    }

    /// This agent's entry for every task, as messages, after confirming its own winning
    /// bids with the current time, as "update time" in the table does.
    fn announce_all(&mut self, now: u64) -> Vec<Message> {
        for claim in &mut self.claims {
            if claim.winner == Some(self.id) {
                claim.time = now;
            }
        }

        self.tasks
            .iter()
            .map(|&task| self.message(task, self.claims[task.index()]))
            .collect()
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

    /// This agent's entry for `task`.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    #[must_use]
    pub fn claim(&self, task: TaskId) -> Claim {
        self.claims[task.index()]
    }

    /// Whether this agent's bundle holds exactly the tasks it believes it wins.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        let won = self
            .claims
            .iter()
            .filter(|claim| claim.winner == Some(self.id))
            .count();

        won == self.bundle.len()
            && self
                .bundle
                .tasks()
                .all(|task| self.claims[task.index()].winner == Some(self.id))
    }

    /// A message from this agent about `task`.
    const fn message(&self, task: TaskId, claim: Claim) -> Message {
        Message {
            sender: self.id,
            task,
            claim,
        }
    }
}

/// Keeps only the last message about each task in `outbox`, in their original order.
fn keep_last_per_task(outbox: &mut Vec<Message>) {
    let mut seen: Vec<TaskId> = Vec::with_capacity(outbox.len());
    let mut kept: Vec<Message> = Vec::with_capacity(outbox.len());
    for message in outbox.drain(..).rev() {
        if !seen.contains(&message.task) {
            seen.push(message.task);
            kept.push(message);
        }
    }

    kept.reverse();
    *outbox = kept;
}

/// Whether agent `id`, bidding `gain`, beats `claim`: a positive bid that is higher, or equal
/// and placed by a smaller agent.
fn wins(id: AgentId, claim: Claim, gain: u64) -> bool {
    gain > 0
        && match claim.winner {
            None => true,
            Some(winner) => gain > claim.bid || (gain == claim.bid && id < winner),
        }
}

impl<V: Valuation<TaskId>> Participant for Agent<V> {
    type Message = Message;

    fn id(&self) -> AgentId {
        self.id
    }

    fn bundle(&self) -> &Bundle<TaskId> {
        &self.bundle
    }

    /// Runs the bundle phase and announces the new bids, plus the whole view when a
    /// re-announcement is due. Of everything queued this round about one task, only the
    /// last message goes out. Under [`Rules::Gnc2010`], a queued claim that names this
    /// agent for a task it no longer holds is dropped, reading section IV.A of the paper:
    /// the outgoing messages "accurately represent the state of the current bundle build".
    fn act(&mut self, now: u64, outbox: &mut Vec<Message>) {
        outbox.extend(self.build_bundle(now));

        if self
            .announce_every
            .is_some_and(|period| now % period == period - 1)
        {
            outbox.extend(self.announce_all(now));
        }

        keep_last_per_task(outbox);
        if self.rules == Rules::Gnc2010 {
            outbox.retain(|message| {
                message.claim.winner != Some(self.id) || self.bundle.contains(&message.task)
            });
        }
    }

    /// Resolves `message` and rebroadcasts right away.
    fn receive(&mut self, now: u64, message: &Message, outbox: &mut Vec<Message>) {
        outbox.extend(Agent::receive(self, now, message));
    }

    /// Every agent believes in the same winner and bid for every task, every bundle
    /// matches, no bundle phase would add a task, and every message in flight names the
    /// agreed winner and bid. Such messages can only move bid times, never winners, so the
    /// assignment is final even while re-announcements keep the network busy. A lost
    /// message can leave a quiet network split, so quiet alone is not enough.
    fn is_settled(team: &[Self], in_flight: &[&Message]) -> bool {
        let Some(first) = team.first() else {
            return true;
        };

        let agreed = |task: TaskId, claim: Claim| {
            let theirs = first.claims[task.index()];
            claim.winner == theirs.winner && claim.bid == theirs.bid
        };

        team.iter().all(Agent::is_consistent)
            && !team.iter().any(Agent::can_bid)
            && team.iter().all(|agent| {
                agent
                    .tasks
                    .iter()
                    .all(|&task| agreed(task, agent.claims[task.index()]))
            })
            && in_flight.iter().all(|message| {
                message.task.index() < first.claims.len() && agreed(message.task, message.claim)
            })
    }
}

/// Unit tests for [`Agent`].
#[cfg(test)]
mod tests;
