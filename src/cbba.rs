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
//! different order. The tests pin both cases, the first with a step-by-step trace. Agents
//! that [replan](Replan::Full), releasing their bundle before every bundle phase, free such
//! slots, and settle on a sequential greedy assignment for some order of tied gains.
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
//!
//! A running team can change: [`Agent::add_task`] adds a task to every agent's tables,
//! [`Agent::retire_task`] withdraws a done or cancelled one, and an agent that
//! [leaves](Agent::leave) is [forgotten](Agent::forget_agent) by the others. Bundles the
//! change does not touch stay as they are.
//!
//! In a frame-driven loop, such as a game's, [`Agent::with_bundle_budget`] caps the tasks one
//! bundle phase adds, so a phase's cost fits a frame and a bundle fills over several rounds.
//! Without replanning, the bundle and consensus phases do not allocate once the bundle has
//! reached its capacity before, including when the agent is outbid; only building a
//! [`Message`] does.
//!
//! By default every agent broadcasts every round, as published. With
//! [`Cadence::OnChange`], an agent only broadcasts after its winners or bids changed, so a
//! team that agrees falls silent; an optional refresh resends the state now and then, for
//! networks that lose messages or partition.
//!
//! For valuations whose marginal gains do not diminish, such as routes where tasks become
//! cheaper together, [`Agent::with_bid_warping`] caps every bid at the one before it in the
//! bundle; see [`warp`].

mod cadence;
mod dynamic;
mod message;
pub(crate) mod table;
pub mod warp;

pub use cadence::Cadence;
pub use message::Message;

use alloc::vec;
use alloc::vec::Vec;

use crate::bundle::Bundle;
use crate::id::{AgentId, TaskId};
use crate::quiet::Quiet;
use crate::valuation::{Insertion, Valuation};
pub(crate) use table::Action;
use table::{Claim, Exchange};

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

    /// How much of its bundle this agent releases before each bundle phase.
    replan: Replan,

    /// The most tasks one bundle phase adds; `None` for no limit.
    budget: Option<usize>,

    /// For each agent, whether this agent has been told it left the team; its claims are
    /// void and its messages ignored.
    departed: Vec<bool>,

    /// For each task, whether it was retired: done or cancelled, never bid on again.
    retired: Vec<bool>,

    /// Whether this agent itself left the team: it holds nothing and stays silent.
    left: bool,

    /// When this agent broadcasts.
    cadence: Cadence,

    /// The winner tables of this agent's last message; empty before the first.
    sent: Vec<Claim>,

    /// Rounds since this agent's last message in which it stayed silent.
    silent_rounds: u64,

    /// Whether this agent [warps its bids](Agent::with_bid_warping), so that they never
    /// grow along its bundle.
    warping: bool,

    /// How many bundle phases in a row this agent's winner tables stayed the same.
    quiet: Quiet<Claim>,
}

/// How much of its bundle an agent releases before each bundle phase, to reconsider it.
///
/// With [`Replan::Keep`], an agent only ever adds to its bundle, so a slot it filled while
/// a since-withdrawn bid blocked a better task stays filled. Releasing the bundle and
/// rebuilding it against the current winners frees such slots. With diminishing marginal
/// gains and every task known from the start, the team then settles on the
/// [sequential greedy assignment](crate::centralized::sequential_greedy), up to the order of
/// tied gains (Buckman, Choi and How, "Partial Replanning for Decentralized Dynamic Task
/// Allocation", AIAA `SciTech` 2019, <https://arxiv.org/abs/1806.04836>).
///
/// Releasing only the tasks added last trades that coordination for fewer bundle changes
/// per round, and fewer valuation calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Replan {
    /// Keep the bundle and only add to it: CBBA as published in 2009.
    #[default]
    Keep,

    /// Release the whole bundle and rebuild it from scratch.
    Full,

    /// Release the given number of tasks added last, which under diminishing gains carry
    /// the lowest bids, and rebuild from there.
    Last(usize),
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
            replan: Replan::Keep,
            budget: None,
            departed: vec![false; agents],
            retired: vec![false; tasks],
            left: false,
            cadence: Cadence::EveryRound,
            sent: Vec::new(),
            silent_rounds: 0,
            warping: false,
            quiet: Quiet::new(core::iter::repeat_n(Claim::NONE, tasks)),
        }
    }

    /// This agent, releasing part of its bundle before every bundle phase as `replan` says.
    ///
    /// The default, [`Replan::Keep`], is CBBA as published in 2009.
    ///
    /// # Panics
    ///
    /// Panics when a [bundle budget](Agent::with_bundle_budget) below the capacity is set and
    /// `replan` releases as much per phase as the budget re-adds, or more: [`Replan::Full`],
    /// or [`Replan::Last`] of the budget or more.
    #[must_use]
    pub const fn with_replan(mut self, replan: Replan) -> Self {
        assert!(
            budget_fits(replan, self.budget, self.capacity),
            "a bundle budget below the capacity cannot re-add what this replan releases"
        );
        self.replan = replan;
        self
    }

    /// This agent, adding at most `additions` tasks per bundle phase, so that the phase's
    /// work fits a frame: each added task costs one pass over every task the agent can still
    /// win. A bundle then fills over several rounds, and the team keeps negotiating meanwhile.
    ///
    /// The limit counts tasks added after the release. A budget below the capacity
    /// therefore only works with [`Replan::Keep`], or with [`Replan::Last`] of fewer than
    /// `additions`: a replan that releases as many or more would stop the bundle from growing
    /// for good, while the team counts as settled.
    ///
    /// # Panics
    ///
    /// Panics when `additions` is zero, since the agent could never bid, and when it is below
    /// the capacity while the agent replans as much as it allows or more ([`Replan::Full`],
    /// or [`Replan::Last`] of `additions` or more).
    #[must_use]
    pub fn with_bundle_budget(mut self, additions: usize) -> Self {
        assert!(
            additions > 0,
            "a bundle phase must be allowed to add a task"
        );
        assert!(
            budget_fits(self.replan, Some(additions), self.capacity),
            "a bundle budget below the capacity cannot re-add what this replan releases"
        );
        self.budget = Some(additions);
        self
    }

    /// The bundle phase: releases what [`Replan`] says, then adds the most valuable tasks
    /// this agent can win until its bundle is full or nothing is left to win; returns the
    /// number of tasks added after the release.
    ///
    /// Each added task's bid is its marginal gain, capped at the previous bid when this
    /// agent [warps its bids](Agent::with_bid_warping), and this agent becomes its winner in
    /// its own tables. With [`Replan::Keep`], the bundle grows in place, without allocating once
    /// the bundle has reached its capacity before.
    ///
    /// Each call counts as a round for [`quiet_rounds`](Agent::quiet_rounds).
    pub fn build_bundle(&mut self) -> usize {
        let added = self.bundle_phase();
        self.quiet.observe(self.claims.iter().copied());

        added
    }

    /// The bundle phase proper, as [`build_bundle`](Agent::build_bundle) describes it.
    fn bundle_phase(&mut self) -> usize {
        if self.replan == Replan::Keep {
            let limit = self.limit(self.bundle.len());
            let Self {
                id,
                valuation,
                bundle,
                claims,
                tasks,
                warping,
                ..
            } = self;

            return extend_and_claim(*id, valuation, tasks, limit, *warping, bundle, claims);
        }

        let (bundle, claims, added) = self.rebuilt();
        self.bundle = bundle;
        self.claims = claims;

        added
    }

    /// Whether the bundle phase would change anything now: this agent would add a task, or,
    /// when it replans, rebuild its bundle differently.
    ///
    /// With [`Replan::Keep`] this looks for one winnable task. Otherwise it runs a whole
    /// bundle phase on a copy, as costly as [`build_bundle`](Agent::build_bundle).
    #[must_use]
    pub fn can_bid(&self) -> bool {
        if self.replan == Replan::Keep {
            let id = self.id;
            let claims = &self.claims;
            let ceiling = warp::ceiling(&self.bundle, self.warping);

            return self.bundle.len() < self.capacity
                && self
                    .bundle
                    .best_candidate(&self.valuation, &self.tasks, |task, gain| {
                        Self::wins(id, claims, *task, gain.min(ceiling))
                    })
                    .is_some();
        }

        let (bundle, claims, _) = self.rebuilt();

        bundle != self.bundle || claims != self.claims
    }

    /// The bundle and winner tables the bundle phase would produce, and how many tasks it
    /// would add after the release.
    fn rebuilt(&self) -> (Bundle<TaskId>, Vec<Claim>, usize) {
        let mut bundle = self.bundle.clone();
        let mut claims = self.claims.clone();
        self.release_for_replan(&mut bundle, &mut claims);

        let added = extend_and_claim(
            self.id,
            &self.valuation,
            &self.tasks,
            self.limit(bundle.len()),
            self.warping,
            &mut bundle,
            &mut claims,
        );

        (bundle, claims, added)
    }

    /// The bundle size this bundle phase may reach, starting from `held` tasks: the
    /// capacity, or fewer when the budget allows fewer additions.
    fn limit(&self, held: usize) -> usize {
        self.budget.map_or(self.capacity, |additions| {
            self.capacity.min(held.saturating_add(additions))
        })
    }

    /// Releases the tasks [`Replan`] says from `bundle`, forgetting this agent's own bids on
    /// them in `claims`.
    fn release_for_replan(&self, bundle: &mut Bundle<TaskId>, claims: &mut [Claim]) {
        let keep = match self.replan {
            Replan::Keep => return,
            Replan::Full => 0,
            Replan::Last(count) => bundle.len().saturating_sub(count),
        };

        for task in bundle.release_from(keep) {
            let claim = &mut claims[task.index()];
            if claim.winner == Some(self.id) {
                *claim = Claim::NONE;
            }
        }
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
        self.sent.clone_from(&self.claims);
        self.silent_rounds = 0;

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
    /// Messages from this agent itself, from agents outside the team or [departed](Agent::forget_agent),
    /// or older than what this agent has already seen of the sender are ignored. A message
    /// about fewer or more tasks than this agent knows is merged over the tasks both know.
    pub fn receive(&mut self, message: &Message) -> bool {
        self.receive_with_actions(message, |_, _| {})
            .unwrap_or(false)
    }

    /// The consensus phase for one message, as [`receive`](Agent::receive), also reporting
    /// the table's action for every task to `on_action`; `None` when the message is ignored.
    pub(crate) fn receive_with_actions(
        &mut self,
        message: &Message,
        on_action: impl FnMut(TaskId, Action),
    ) -> Option<bool> {
        if !self.accepts(message) {
            return None;
        }

        let mut changed = self.merge_claims(message, on_action);
        self.merge_seen(message);
        changed |= self.release_outbid();

        Some(changed)
    }

    /// Whether `message` is worth merging: from another member of the team, about the same
    /// tasks, and not older than what this agent has seen of the sender.
    fn accepts(&self, message: &Message) -> bool {
        let sender = message.sender.index();

        message.sender != self.id
            && sender < self.seen.len()
            && !self.departed[sender]
            && message.seen.len() == self.seen.len()
            && message.seen[sender] >= self.seen[sender]
    }

    /// Applies the conflict-resolution table to every task both sides know, reporting each
    /// action to `on_action`; returns whether any claim changed. Claims of retired tasks and
    /// of departed agents are voided, however they arrive.
    fn merge_claims(
        &mut self,
        message: &Message,
        mut on_action: impl FnMut(TaskId, Action),
    ) -> bool {
        let exchange = Exchange {
            receiver: self.id,
            sender: message.sender,
            sender_seen: &message.seen,
            receiver_seen: &self.seen,
        };

        let (departed, retired) = (&self.departed, &self.retired);
        let mut changed = false;
        for (index, (held, &sent)) in self.claims.iter_mut().zip(&message.claims).enumerate() {
            let action = exchange.resolve(sent, *held);
            let next = match action {
                Action::Update => sent,
                Action::Reset => Claim::NONE,
                Action::Leave => *held,
            };
            let void = retired[index] || next.winner.is_some_and(|winner| departed[winner.index()]);
            let next = if void { Claim::NONE } else { next };

            changed |= next != *held;
            *held = next;
            on_action(TaskId(index), action);
        }

        changed
    }

    /// Takes in the freshest clock values the sender knows of.
    fn merge_seen(&mut self, message: &Message) {
        for (mine, &theirs) in self.seen.iter_mut().zip(&message.seen) {
            *mine = (*mine).max(theirs);
        }
    }

    /// Gives up `task` and every task added after it, forgetting this agent's own bids on
    /// all of them; returns whether `task` was in the bundle.
    pub(crate) fn give_up(&mut self, task: TaskId) -> bool {
        let Some(index) = self.bundle.index_of(&task) else {
            return false;
        };

        self.release_own(index);

        true
    }

    /// Releases every task added at bundle index `index` or later, forgetting this agent's
    /// own bids on them, without allocating.
    fn release_own(&mut self, index: usize) {
        let Self {
            id, bundle, claims, ..
        } = self;

        bundle.release_from_with(index, |task| {
            let claim = &mut claims[task.index()];
            if claim.winner == Some(*id) {
                *claim = Claim::NONE;
            }
        });
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

        // The lost task already names another winner, so only the later ones are forgotten.
        self.release_own(lost);

        true
    }
}

/// Whether a bundle `budget` lets every bundle phase add more than `replan` releases, so
/// that bundles can still reach `capacity`.
const fn budget_fits(replan: Replan, budget: Option<usize>, capacity: usize) -> bool {
    let Some(budget) = budget else {
        return true;
    };

    budget >= capacity
        || match replan {
            Replan::Keep => true,
            Replan::Full => false,
            Replan::Last(released) => released < budget,
        }
}

/// Extends `bundle` greedily with the tasks agent `id` can win against `claims`, up to
/// `capacity`, and records the agent as the winner of each added task, at its bid; returns
/// the number of tasks added.
///
/// Each step takes the task with the highest marginal gain whose bid wins. The bid is the
/// gain, capped with `warping` at the bid on the task added last.
fn extend_and_claim<V: Valuation<TaskId>>(
    id: AgentId,
    valuation: &V,
    tasks: &[TaskId],
    capacity: usize,
    warping: bool,
    bundle: &mut Bundle<TaskId>,
    claims: &mut [Claim],
) -> usize {
    let before = bundle.len();
    while bundle.len() < capacity {
        let ceiling = warp::ceiling(bundle, warping);
        let Some((&task, insertion)) = bundle.best_candidate(valuation, tasks, |task, gain| {
            Agent::<V>::wins(id, claims, *task, gain.min(ceiling))
        }) else {
            break;
        };

        let bid = insertion.gain.min(ceiling);
        bundle.insert(
            task,
            Insertion {
                gain: bid,
                ..insertion
            },
        );
    }

    for (task, gain) in bundle.entries().skip(before) {
        claims[task.index()] = Claim {
            winner: Some(id),
            bid: gain,
        };
    }

    bundle.len() - before
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

    /// How many bundle phases in a row this agent's winners and bids stayed the same,
    /// counting changes from messages, team changes and its own bids.
    #[must_use]
    pub const fn quiet_rounds(&self) -> u64 {
        self.quiet.rounds()
    }

    /// This agent's own guess that the team has settled: its winners and bids stayed the
    /// same for at least `span` bundle phases, the *separation time* of ACBBA's sources
    /// (Johnson's 2012 MS thesis, pp. 82–84).
    ///
    /// Unlike [`Participant::is_settled`](crate::sim::Participant::is_settled), which sees
    /// the whole team and every message in flight, this needs nothing but the agent, so a
    /// deployed agent can use it. It is a heuristic and can be wrong both ways. A lost
    /// message, a partition or a slow relay looks just like agreement, so it can report
    /// settled too early; Rantanen et al. (IEEE SAM 2018) found ACBBA agents taking lost
    /// packets for consensus. And it waits out the whole span after the team agreed. A
    /// safe span exceeds the longest time news takes to cross the network, which grows with
    /// its diameter, delays and losses.
    #[must_use]
    pub const fn is_locally_settled(&self, span: u64) -> bool {
        self.quiet.rounds() >= span
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
