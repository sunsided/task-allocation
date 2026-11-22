//! The consensus-based bundle algorithm (CBBA).
//!
//! Each agent alternates two phases, communicating only with whoever it can currently reach:
//!
//! 1. **Bundle phase** ([`Agent::build_bundle`]): the agent greedily adds tasks to its
//!    bundle, each time the one with the highest marginal gain it can win, bidding that gain.
//!    It can win a task when its bid beats the best bid it knows of: higher, or equal and
//!    placed by an agent with a larger identifier.
//! 2. **Consensus phase** ([`Agent::receive`]): the agent merges a neighbor's [`Message`]
//!    into its own winner tables, task by task, with the published conflict-resolution table.
//!    When it learns it was outbid on a task in its bundle, it drops that task and every task
//!    it added after it, since their bids assumed the lost task.
//!
//! Messages carry the sender's whole state, so they can be delayed, dropped, duplicated or
//! reordered, and the topology can change: an agent ignores a message older than what it
//! already knows of the sender, and later messages repair what an earlier one got wrong.
//!
//! With [diminishing marginal gains](crate::check_diminishing), once every pair of agents
//! keeps getting messages through, directly or relayed, the team settles on an assignment
//! where:
//!
//! - every agent believes in the same winner for every task, and no task has two holders;
//! - every winning bid is the holder's real marginal gain, given the tasks it added before;
//! - no agent with room left could outbid the winner of a task it does not hold.
//!
//! The literature states that, with diminishing gains, CBBA settles on the
//! [sequential greedy assignment](crate::centralized::sequential_greedy) (Choi, Brunet and
//! How, as restated by Buckman, Choi and How, 2019). With this implementation that often
//! holds, but not always: an agent outbid by a bid that is later withdrawn may already have
//! filled the slot with another task, and keeps it; and equal gains can be resolved in a
//! different order. The tests pin both cases, the first with a step-by-step trace.
//!
//! The algorithm is from Choi, Brunet and How, "Consensus-Based Decentralized Auctions for
//! Robust Task Allocation", IEEE Transactions on Robotics 25(4), 2009. Its state and release
//! rule follow the restatement in Section II.B of Johnson, Ponda, Choi and How,
//! "Asynchronous Decentralized Task Allocation for Dynamic Environments", AIAA
//! Infotech@Aerospace 2011, and its conflict table was checked entry by entry against the
//! open-source CBBA-Python implementation (<https://github.com/zehuilu/CBBA-Python>). Timestamps here are
//! logical clocks: every agent counts the messages it sends, and agents track the latest count
//! they have seen of each other agent, directly or relayed. No synchronized clocks are needed.
//!
//! ```mermaid
//! sequenceDiagram
//!     participant A as agent 0
//!     participant B as agent 1
//!     A->>A: build_bundle
//!     B->>B: build_bundle
//!     A->>B: message
//!     B->>A: message
//!     A->>A: receive, release outbid tasks
//!     B->>B: receive, release outbid tasks
//!     A->>A: build_bundle
//! ```
//!
//! [`sim`](crate::sim) runs a team over a simulated network with delays, losses and
//! partitions.

mod message;
pub(crate) mod table;

pub use message::Message;

use alloc::vec;
use alloc::vec::Vec;

use crate::bundle::Bundle;
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;
use table::{Action, Claim, Exchange};

/// One CBBA agent: its valuation, its bundle, and what it knows about every task's winner.
///
/// # Examples
///
/// Two agents that can each take one task, exchanging messages directly:
///
/// ```
/// use task_allocation::{AgentId, TaskId, Valuation, cbba::Agent};
///
/// /// Values the two tasks at fixed rewards, for at most one task.
/// struct Single([u64; 2]);
///
/// impl Valuation<TaskId> for Single {
///     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
///         path.is_empty().then_some(self.0[task.index()])
///     }
/// }
///
/// let mut a = Agent::new(AgentId(0), 2, 2, 1, Single([5, 4]));
/// let mut b = Agent::new(AgentId(1), 2, 2, 1, Single([6, 1]));
///
/// for _ in 0..3 {
///     a.build_bundle();
///     b.build_bundle();
///     let (to_b, to_a) = (a.message(), b.message());
///     b.receive(&to_b);
///     a.receive(&to_a);
/// }
///
/// assert_eq!(a.bundle().path(), [TaskId(1)]);
/// assert_eq!(b.bundle().path(), [TaskId(0)]);
/// assert_eq!(a.winner(TaskId(0)), Some(AgentId(1)));
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

    /// For each task, the winner and winning bid this agent believes in.
    claims: Vec<Claim>,

    /// For each agent, the latest clock value of it this agent has seen, directly or
    /// relayed. The entry for this agent is its own clock: the number of messages it sent.
    seen: Vec<u64>,

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
        assert!(
            id.index() < agents,
            "{id} is not part of a team of {agents} agents"
        );

        Self {
            id,
            valuation,
            capacity,
            bundle: Bundle::new(),
            claims: vec![Claim::NONE; tasks],
            seen: vec![0; agents],
            tasks: (0..tasks).map(TaskId).collect(),
        }
    }

    /// The bundle phase: adds the most valuable tasks this agent can win until its bundle is
    /// full or nothing is left to win; returns the number of tasks added.
    ///
    /// Each added task's bid is its marginal gain, and this agent becomes its winner in its
    /// own tables.
    pub fn build_bundle(&mut self) -> usize {
        let id = self.id;
        let claims = &self.claims;
        let before = self.bundle.len();
        self.bundle
            .extend_greedily(&self.valuation, &self.tasks, self.capacity, |task, gain| {
                Self::wins(id, claims, *task, gain)
            });

        for (task, gain) in self.bundle.entries().skip(before) {
            self.claims[task.index()] = Claim {
                winner: Some(id),
                bid: gain,
            };
        }

        self.bundle.len() - before
    }

    /// Whether the bundle phase would add a task now: this agent has room left and can
    /// outbid the best known bid on some task.
    #[must_use]
    pub fn can_bid(&self) -> bool {
        let id = self.id;
        let claims = &self.claims;

        self.bundle.len() < self.capacity
            && self
                .bundle
                .best_candidate(&self.valuation, &self.tasks, |task, gain| {
                    Self::wins(id, claims, *task, gain)
                })
                .is_some()
    }

    /// Whether agent `id`, bidding `gain` on `task`, beats the best bid in `claims`.
    fn wins(id: AgentId, claims: &[Claim], task: TaskId, gain: u64) -> bool {
        gain > 0
            && Claim {
                winner: Some(id),
                bid: gain,
            }
            .outbids(claims[task.index()])
    }

    /// The message this agent broadcasts now: its winner tables and what it has seen of
    /// every agent. Advances this agent's clock.
    #[must_use]
    pub fn message(&mut self) -> Message {
        self.seen[self.id.index()] += 1;

        Message {
            sender: self.id,
            claims: self.claims.clone(),
            seen: self.seen.clone(),
        }
    }

    /// The consensus phase for one message: merges `message` into this agent's winner
    /// tables, and drops the tasks it was outbid on along with every task added after them.
    /// Returns whether this agent's tables or bundle changed.
    ///
    /// Messages from this agent itself, from agents outside the team, of a different task
    /// count, or older than what this agent has already seen of the sender are ignored.
    pub fn receive(&mut self, message: &Message) -> bool {
        if !self.accepts(message) {
            return false;
        }

        let mut changed = self.merge_claims(message);
        self.merge_seen(message);
        changed |= self.release_outbid();

        changed
    }

    /// Whether `message` is worth merging: from another member of the team, about the same
    /// tasks, and not older than what this agent has seen of the sender.
    fn accepts(&self, message: &Message) -> bool {
        let sender = message.sender.index();

        message.sender != self.id
            && sender < self.seen.len()
            && message.seen.len() == self.seen.len()
            && message.claims.len() == self.claims.len()
            && message.seen[sender] >= self.seen[sender]
    }

    /// Applies the conflict-resolution table to every task; returns whether any claim
    /// changed.
    fn merge_claims(&mut self, message: &Message) -> bool {
        let exchange = Exchange {
            receiver: self.id,
            sender: message.sender,
            sender_seen: &message.seen,
            receiver_seen: &self.seen,
        };

        let mut changed = false;
        for (held, &sent) in self.claims.iter_mut().zip(&message.claims) {
            let next = match exchange.resolve(sent, *held) {
                Action::Update => sent,
                Action::Reset => Claim::NONE,
                Action::Leave => *held,
            };

            changed |= next != *held;
            *held = next;
        }

        changed
    }

    /// Takes in the freshest clock values the sender knows of.
    fn merge_seen(&mut self, message: &Message) {
        for (mine, &theirs) in self.seen.iter_mut().zip(&message.seen) {
            *mine = (*mine).max(theirs);
        }
    }

    /// Drops the first task in the bundle this agent no longer wins, and everything added
    /// after it, forgetting its own bids on the later ones; returns whether anything was
    /// dropped.
    fn release_outbid(&mut self) -> bool {
        let Some(lost) = self
            .bundle
            .tasks()
            .position(|task| self.claims[task.index()].winner != Some(self.id))
        else {
            return false;
        };

        for task in self.bundle.release_from(lost).into_iter().skip(1) {
            let claim = &mut self.claims[task.index()];
            if claim.winner == Some(self.id) {
                *claim = Claim::NONE;
            }
        }

        true
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

    /// The winner this agent believes in for `task`; `None` when it knows of no bid.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    #[must_use]
    pub fn winner(&self, task: TaskId) -> Option<AgentId> {
        self.claims[task.index()].winner
    }

    /// The winning bid this agent believes in for `task`; zero when it knows of no bid.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    #[must_use]
    pub fn winning_bid(&self, task: TaskId) -> u64 {
        self.claims[task.index()].bid
    }

    /// The winner this agent believes in for every task, in task order.
    pub fn winners(&self) -> impl ExactSizeIterator<Item = Option<AgentId>> + '_ {
        self.claims.iter().map(|claim| claim.winner)
    }

    /// The winner and winning bid this agent believes in for every task, in task order.
    pub(crate) fn claims(&self) -> &[Claim] {
        &self.claims
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
}

/// Unit tests for [`Agent`].
#[cfg(test)]
mod tests;
