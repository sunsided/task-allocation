//! Coupled-constraint CBBA (CCBBA): CBBA for tasks that need, or exclude, each other.
//!
//! Some tasks only make sense together, such as a strike that needs a sensor, and some
//! exclude each other. [`Constraints`] describes these relations in activities. An
//! [`Agent`] runs [CBBA](crate::cbba) with the extensions of Whitten, Choi, Johnson and How,
//! "Decentralized Task Allocation with Coupled Constraints in Complex Missions", American
//! Control Conference 2011:
//!
//! - **Pessimistic bidding**: an agent bids on a task only once every task it depends on has
//!   a winner. Tasks with no mutual dependency are bid on this way.
//! - **Optimistic bidding**: on a task with at least one mutual dependency, an agent may
//!   bid first and wait for its partners, for all its dependencies, one-way ones included,
//!   as in the paper (`O_jq` non-empty). If they do not appear within [`Optimism::timeout`] rounds, it gives
//!   the task up and loses one of a limited number of attempts. Once the attempts are used
//!   up, it bids pessimistically.
//! - **Mutual exclusion**: an agent bids on a task only if it outbids every task excluded by
//!   it, and gives a task up once an excluded task's winner outbids it.
//!
//! Each round, an agent first gives up the tasks whose constraints fail, then runs the
//! bundle phase; messages and the conflict table are CBBA's.
//!
//! The paper promises no convergence, and there is none in general: constrained gains do
//! not diminish, by design. An agent can keep outbidding a partner on a task whose
//! dependency only that partner's bundle provides, so the partner releases the dependency,
//! the agent gives the task up, the partner rebuilds both, and the cycle repeats. The tests
//! pin such a case. The timeouts and attempt counts are the caller's choice; the paper
//! gives no values.
//!
//! Choices this implementation makes where the paper is silent:
//!
//! - Eligibility is decided at the start of each bundle phase, from the winners known then.
//!   A task that depends on another the agent adds in the same phase becomes eligible in the
//!   next round.
//! - The count of rounds an optimistic task waited resets once its dependencies are met.
//! - Ties in mutual exclusion go to the smaller agent, as everywhere in this crate; a strict
//!   comparison would make both tasks give way.

mod constraints;
mod timed;

pub use constraints::{ConstraintError, Constraints};
pub use timed::{TimedAgent, TimedMessage, Windowed};

use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::bundle::Bundle;
use crate::cbba::table::Claim;
use crate::cbba::{self, Message};
use crate::id::{AgentId, TaskId};
use crate::sim::Participant;
use crate::valuation::{Insertion, Valuation};

/// How long, and how often, an agent bids optimistically on a task in a mutual dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Optimism {
    /// Rounds an agent keeps such a task while a task it depends on has no winner; `o` in
    /// the paper.
    pub timeout: u32,

    /// Attempts at bidding on such a task even while none of the tasks it depends on has a
    /// winner; `w_solo` in the paper.
    pub solo_attempts: u32,

    /// Attempts at bidding on such a task once at least one of the tasks it depends on has
    /// a winner; `w_any` in the paper, usually larger than `solo_attempts`.
    ///
    /// As in the paper, an agent may bid while either budget lasts (`any` only with a
    /// dependency met), and every timeout spends one attempt from both.
    pub any_attempts: u32,
}

/// What a [`Gate`] knows when deciding whether its agent may bid on a task.
#[derive(Clone, Debug)]
struct View {
    /// For each task, the winner and winning bid as of the start of the bundle phase.
    claims: Vec<Claim>,

    /// For each task, the remaining solo attempts.
    solo: Vec<u32>,

    /// For each task, the remaining attempts with some dependencies met.
    any: Vec<u32>,

    /// For each task, the start time of its winning bid, when known; always `None` for
    /// agents without timing.
    starts: Vec<Option<u64>>,
}

/// A valuation that hides tasks the agent may not bid on, so CBBA's bundle phase skips them.
#[derive(Clone, Debug)]
struct Gate<V> {
    /// The agent's own valuation.
    inner: V,

    /// The constraints between tasks.
    constraints: Constraints,

    /// The agent this gate belongs to.
    id: AgentId,

    /// The state eligibility depends on, refreshed before every bundle phase.
    view: RefCell<View>,
}

impl<V> Gate<V> {
    /// The agent's own valuation.
    const fn inner(&self) -> &V {
        &self.inner
    }

    /// The constraints between tasks.
    const fn constraints(&self) -> &Constraints {
        &self.constraints
    }

    /// How many of `task`'s dependencies have a winner, and how many it has.
    fn satisfied(&self, claims: &[Claim], task: TaskId) -> (usize, usize) {
        let mut met = 0;
        let mut required = 0;
        for requirement in self.constraints.requirements(task) {
            required += 1;
            if claims[requirement.index()].winner.is_some() {
                met += 1;
            }
        }

        (met, required)
    }

    /// Whether the agent has optimistic attempts left at `task`; without, it bids, and
    /// releases, pessimistically.
    fn has_attempts(&self, task: TaskId) -> bool {
        let view = self.view.borrow();

        view.solo[task.index()] > 0 || view.any[task.index()] > 0
    }

    /// Whether the agent may bid on `task` at all, before looking at its gain.
    fn may_bid(&self, task: TaskId) -> bool {
        let view = self.view.borrow();
        let (met, required) = self.satisfied(&view.claims, task);
        if met == required {
            return true;
        }

        self.constraints.is_mutual(task)
            && (view.solo[task.index()] > 0 || (view.any[task.index()] > 0 && met > 0))
    }

    /// Whether a bid of `gain` on `task` outbids every task excluded by it.
    fn beats_exclusions(&self, task: TaskId, gain: u64) -> bool {
        let view = self.view.borrow();
        let bid = Claim {
            winner: Some(self.id),
            bid: gain,
        };

        self.constraints.exclusions(task).all(|other| {
            view.claims[other.index()].winner.is_none() || bid.outbids(view.claims[other.index()])
        })
    }
}

impl<V: Valuation<TaskId>> Valuation<TaskId> for Gate<V> {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        self.best_insertion(bundle, task)
            .map(|insertion| insertion.gain)
    }

    fn best_insertion(&self, path: &[TaskId], task: &TaskId) -> Option<Insertion> {
        if !self.may_bid(*task) {
            return None;
        }

        let insertion = self.inner.best_insertion(path, task)?;

        self.beats_exclusions(*task, insertion.gain)
            .then_some(insertion)
    }
}

/// One CCBBA agent: a CBBA agent whose bids respect coupled constraints.
///
/// # Examples
///
/// ```
/// use task_allocation::ccbba::{Agent, Constraints, Optimism};
/// use task_allocation::sim::{Network, Simulation};
/// use task_allocation::{AgentId, TaskId, Valuation};
///
/// /// A strike (task 0) and the sensing it needs (task 1), each worth something to one agent.
/// struct Fixed([u64; 2]);
///
/// impl Valuation<TaskId> for Fixed {
///     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
///         path.is_empty().then_some(self.0[task.index()])
///     }
/// }
///
/// let mut constraints = Constraints::new(2);
/// constraints.add_activity(&[TaskId(0), TaskId(1)], &[&[0, 1], &[1, 0]]).unwrap();
/// let optimism = Optimism { timeout: 5, solo_attempts: 2, any_attempts: 4 };
///
/// let agents = [Fixed([9, 0]), Fixed([0, 3])]
///     .into_iter()
///     .enumerate()
///     .map(|(id, valuation)| Agent::new(AgentId(id), 2, 2, 1, valuation, constraints.clone(), optimism))
///     .collect();
/// let mut simulation = Simulation::new(agents, Network::default());
///
/// simulation.run_until_settled(100).expect("settles");
/// let assignment = simulation.assignment();
/// assert_eq!(assignment.holder(TaskId(0)), Some(AgentId(0)));
/// assert_eq!(assignment.holder(TaskId(1)), Some(AgentId(1)));
/// assert!(constraints.is_satisfied_by(&assignment));
/// ```
#[derive(Clone, Debug)]
pub struct Agent<V> {
    /// The CBBA agent doing the bidding and consensus.
    inner: cbba::Agent<Gate<V>>,

    /// How long and how often this agent bids optimistically.
    optimism: Optimism,

    /// For each task, the rounds this agent has held it while a dependency had no winner;
    /// `ν` in the paper.
    waited: Vec<u32>,
}

impl<V: Valuation<TaskId>> Agent<V> {
    /// Agent `id` of a team of `agents`, bidding on tasks `0..tasks` under `constraints` and
    /// holding at most `capacity` of them.
    ///
    /// # Panics
    ///
    /// Panics when `id` is not below `agents`, or when `constraints` cover a different
    /// number of tasks.
    #[must_use]
    pub fn new(
        id: AgentId,
        agents: usize,
        tasks: usize,
        capacity: usize,
        valuation: V,
        constraints: Constraints,
        optimism: Optimism,
    ) -> Self {
        assert_eq!(
            constraints.tasks(),
            tasks,
            "constraints must cover exactly the tasks"
        );

        let gate = Gate {
            inner: valuation,
            constraints,
            id,
            view: RefCell::new(View {
                claims: vec![Claim::NONE; tasks],
                solo: vec![optimism.solo_attempts; tasks],
                any: vec![optimism.any_attempts; tasks],
                starts: vec![None; tasks],
            }),
        };

        Self {
            inner: cbba::Agent::new(id, agents, tasks, capacity, gate),
            optimism,
            waited: vec![0; tasks],
        }
    }

    /// One round: gives up the tasks whose constraints fail, then runs the bundle phase.
    /// Returns the number of tasks the bundle phase added.
    pub fn build_bundle(&mut self) -> usize {
        self.enforce();
        self.refresh_view();

        let added = self.inner.build_bundle();
        self.refresh_view();

        added
    }

    /// The message this agent broadcasts now; see [`cbba::Agent::message`].
    #[must_use]
    pub fn message(&mut self) -> Message {
        self.inner.message()
    }

    /// The consensus phase for one message; see [`cbba::Agent::receive`].
    pub fn receive(&mut self, message: &Message) -> bool {
        let changed = self.inner.receive(message);
        self.refresh_view();

        changed
    }

    /// Whether the next round would change anything: a task would be given up, an
    /// optimistic task is still waiting for its partners, or the bundle phase would add a
    /// task.
    #[must_use]
    pub fn can_bid(&self) -> bool {
        self.violation().is_some() || self.is_waiting() || self.inner.can_bid()
    }

    /// Whether a held task has a dependency without a winner.
    fn is_waiting(&self) -> bool {
        let gate = self.inner.valuation();
        let claims = self.inner.claims();

        self.inner.bundle().tasks().any(|&task| {
            let (met, required) = gate.satisfied(claims, task);
            met < required
        })
    }

    /// Copies the current winners into the gate, which decides eligibility from them.
    fn refresh_view(&self) {
        let mut view = self.inner.valuation().view.borrow_mut();
        view.claims.clear();
        view.claims.extend_from_slice(self.inner.claims());
    }

    /// Gives up tasks whose constraints fail until none do, counting the rounds optimistic
    /// tasks wait for their partners.
    fn enforce(&mut self) {
        self.count_waiting();

        while let Some((task, timed_out)) = self.violation() {
            self.inner.give_up(task);
            self.waited[task.index()] = 0;
            if timed_out {
                self.spend_attempt(task);
            }

            self.refresh_view();
        }
    }

    /// Counts one more round for every held task with a dependency lacking a winner, and
    /// resets the count of every other task, including tasks released since the last round,
    /// however they were released.
    fn count_waiting(&mut self) {
        let gate = self.inner.valuation();
        let claims = self.inner.claims();
        let bundle = self.inner.bundle();
        for (index, waited) in self.waited.iter_mut().enumerate() {
            let task = TaskId(index);
            if !bundle.contains(&task) {
                *waited = 0;
                continue;
            }

            let (met, required) = gate.satisfied(claims, task);
            *waited = if met < required { *waited + 1 } else { 0 };
        }
    }

    /// The first held task whose constraints fail, and whether it is an optimistic task that
    /// waited too long; `None` when all hold.
    ///
    /// A task fails when a dependency has no winner and it is pessimistic or timed out, or
    /// when the winner of an excluded task outbids it.
    fn violation(&self) -> Option<(TaskId, bool)> {
        let gate = self.inner.valuation();
        let claims = self.inner.claims();

        self.inner.bundle().tasks().find_map(|&task| {
            let (met, required) = gate.satisfied(claims, task);
            let unmet = met < required;
            let optimistic = gate.constraints.is_mutual(task) && gate.has_attempts(task);
            let timed_out = self.waited[task.index()] >= self.optimism.timeout;
            if unmet && (!optimistic || timed_out) {
                return Some((task, optimistic));
            }

            let mine = claims[task.index()];
            let outbid = gate.constraints.exclusions(task).any(|other| {
                claims[other.index()].winner.is_some() && !mine.outbids(claims[other.index()])
            });
            if outbid {
                return Some((task, false));
            }

            self.yields_in_time(task).then_some((task, optimistic))
        })
    }

    /// Whether this agent must give up `task` because its start conflicts with the start of
    /// another assigned task in its activity.
    ///
    /// A task that depends on the other one alone gives way; one the other depends on alone
    /// stays. Of two mutually dependent tasks, the one that starts earlier into its time
    /// window gives way, since it could have started later; ties go to the smaller holder,
    /// then the smaller task.
    fn yields_in_time(&self, task: TaskId) -> bool {
        let gate = self.inner.valuation();
        let view = gate.view.borrow();
        let claims = self.inner.claims();
        let constraints = &gate.constraints;
        let Some(start) = view.starts[task.index()] else {
            return false;
        };

        let mine = (
            offset(constraints, task, start),
            core::cmp::Reverse(self.inner.id()),
            core::cmp::Reverse(task),
        );
        constraints.activity_tasks(task).any(|other| {
            let (Some(holder), Some(other_start)) =
                (claims[other.index()].winner, view.starts[other.index()])
            else {
                return false;
            };

            if constraints.starts_agree(task, start, other, other_start) {
                return false;
            }

            match (
                constraints.depends_on(task, other),
                constraints.depends_on(other, task),
            ) {
                (true, false) => true,
                (true, true) => {
                    let theirs = (
                        offset(constraints, other, other_start),
                        core::cmp::Reverse(holder),
                        core::cmp::Reverse(other),
                    );
                    mine < theirs
                }
                _ => false,
            }
        })
    }

    /// Uses up one solo and one any attempt at `task`.
    fn spend_attempt(&self, task: TaskId) {
        let mut view = self.inner.valuation().view.borrow_mut();
        let solo = &mut view.solo[task.index()];
        *solo = solo.saturating_sub(1);
        let any = &mut view.any[task.index()];
        *any = any.saturating_sub(1);
    }
}

/// How far into its time window `task` starts at `start`; negative before the opening.
fn offset(constraints: &Constraints, task: TaskId, start: u64) -> i128 {
    i128::from(start) - i128::from(constraints.opening(task))
}

impl<V> Agent<V> {
    /// This agent's identifier.
    #[must_use]
    pub const fn id(&self) -> AgentId {
        self.inner.id()
    }

    /// This agent's own valuation.
    #[must_use]
    pub const fn valuation(&self) -> &V {
        self.inner.valuation().inner()
    }

    /// The constraints this agent bids under.
    #[must_use]
    pub const fn constraints(&self) -> &Constraints {
        self.inner.valuation().constraints()
    }

    /// The tasks this agent holds, with the bid it placed on each.
    #[must_use]
    pub const fn bundle(&self) -> &Bundle<TaskId> {
        self.inner.bundle()
    }

    /// The winner this agent believes in for `task`; `None` when it knows of no bid.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    #[must_use]
    pub fn winner(&self, task: TaskId) -> Option<AgentId> {
        self.inner.winner(task)
    }

    /// The winning bid this agent believes in for `task`; zero when it knows of no bid.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    #[must_use]
    pub fn winning_bid(&self, task: TaskId) -> u64 {
        self.inner.winning_bid(task)
    }
}

impl<V: Valuation<TaskId>> Participant for Agent<V> {
    type Message = Message;

    fn id(&self) -> AgentId {
        self.inner.id()
    }

    fn bundle(&self) -> &Bundle<TaskId> {
        self.inner.bundle()
    }

    /// Gives up failing tasks, runs the bundle phase, and broadcasts the full state.
    fn act(&mut self, _now: u64, outbox: &mut Vec<Message>) {
        self.build_bundle();
        outbox.push(self.message());
    }

    /// Runs CBBA's consensus phase for `message`.
    fn receive(&mut self, _now: u64, message: &Message, _outbox: &mut Vec<Message>) {
        Agent::receive(self, message);
    }

    /// CBBA's settled check, plus: no agent would give up a task. Then every held task's
    /// dependencies have winners and no excluded task outbids it.
    fn is_settled(team: &[Self], in_flight: &[&Message]) -> bool {
        let Some(first) = team.first() else {
            return true;
        };

        team.iter().all(|agent| agent.inner.is_consistent())
            && !team.iter().any(Agent::can_bid)
            && team
                .iter()
                .all(|agent| agent.inner.claims() == first.inner.claims())
            && in_flight
                .iter()
                .all(|message| message.claims() == first.inner.claims())
    }
}

/// Unit tests for [`Agent`] and [`Constraints`].
#[cfg(test)]
mod tests;
