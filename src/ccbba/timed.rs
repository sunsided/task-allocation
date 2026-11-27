//! CCBBA with temporal constraints: agents on timed routes, whose coupled tasks must start
//! within limits of each other.
//!
//! A [`TimedAgent`] schedules its bundle on a [`Route`]. Every task it holds that depends on
//! another task with a known start gets that task's window ([`Constraints::window`]): the
//! route waits for the window to open and rejects a start after it closes. A path whose
//! own tasks break the timing limits between them is rejected too, so one agent never takes
//! two tasks that must start together. Messages carry the start of every winning bid, adopted whenever the conflict table takes the sender's
//! claim. Each round, an agent also gives up tasks whose starts conflict with their
//! partners', by the rules of Whitten, Choi, Johnson and How (2011); see the
//! [module docs](super).

use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use super::{Agent, Constraints, Optimism};
use crate::bundle::Bundle;
use crate::cbba::{self, Action};
use crate::id::{AgentId, TaskId};
use crate::route::{Route, Timed, Visit};
use crate::sim::Participant;
use crate::valuation::{BestInsertion, PathScore};

/// A route that keeps each task within a window imposed by the starts of its partners.
#[derive(Clone, Debug)]
pub struct Windowed<R> {
    /// The agent's own route.
    route: R,

    /// For each task, the earliest and the latest start the partners allow; `None` for no
    /// latest start.
    windows: RefCell<Vec<(u64, Option<u64>)>>,
}

impl<R> Windowed<R> {
    /// The agent's own route.
    #[must_use]
    pub const fn route(&self) -> &R {
        &self.route
    }
}

impl<R: Route> Route for Windowed<R> {
    type Place = R::Place;

    fn origin(&self) -> R::Place {
        self.route.origin()
    }

    fn departure(&self) -> u64 {
        self.route.departure()
    }

    fn place(&self, task: TaskId) -> R::Place {
        self.route.place(task)
    }

    fn travel(&self, from: &R::Place, to: &R::Place) -> Option<u64> {
        self.route.travel(from, to)
    }

    fn duration(&self, task: TaskId) -> u64 {
        self.route.duration(task)
    }

    /// The route's own earliest start, or the partners' when later.
    fn earliest_start(&self, task: TaskId) -> u64 {
        let (earliest, _) = self.windows.borrow()[task.index()];

        self.route.earliest_start(task).max(earliest)
    }

    /// The route's reward, or `None` when the task would start after its partners allow.
    fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
        let (_, latest) = self.windows.borrow()[task.index()];
        let start = finish.saturating_sub(self.route.duration(task));
        if latest.is_some_and(|latest| start > latest) {
            return None;
        }

        self.route.reward(task, finish)
    }
}

/// A [`TimedAgent`]'s broadcast: CBBA's message, plus the start of every winning bid the
/// sender knows of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedMessage {
    /// Winners, bids and freshness, as in CBBA.
    claims: cbba::Message,

    /// For each task, the start of its winning bid; `None` when unknown or unassigned.
    starts: Vec<Option<u64>>,
}

impl TimedMessage {
    /// The CBBA part: winners, bids and freshness.
    #[must_use]
    pub const fn claims(&self) -> &cbba::Message {
        &self.claims
    }

    /// The start of `task`'s winning bid, as the sender knew it.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of the message's tasks.
    #[must_use]
    pub fn start(&self, task: TaskId) -> Option<u64> {
        self.starts[task.index()]
    }
}

/// The valuation a [`TimedAgent`] bids with: its route, kept within its partners' windows
/// and the timing limits among its own tasks, with each task inserted where it adds the
/// most.
type Scheduled<R> = BestInsertion<Coordinated<R>>;

/// A timed route that also keeps the timing limits among the tasks of one path.
///
/// The partners' windows only cover tasks other agents hold. Within one bundle phase, a
/// task this agent has just added would not narrow the window of the next one, so without
/// this check one agent could take two tasks that must start together, at different
/// starts, and give one up again in every round.
#[derive(Clone, Debug)]
struct Coordinated<R> {
    /// The route within the partners' windows.
    timed: Timed<Windowed<R>>,

    /// The constraints whose timing limits the path must keep.
    constraints: Constraints,
}

impl<R: Route> Coordinated<R> {
    /// Whether `path`, with `task` if given, holds two tasks of one activity, whose starts
    /// then need checking against each other.
    fn links(&self, path: &[TaskId], task: Option<TaskId>) -> bool {
        let tasks = || path.iter().copied().chain(task);

        tasks().any(|member| {
            self.constraints
                .activity_tasks(member)
                .any(|partner| tasks().any(|other| other == partner))
        })
    }
}

impl<R: Route> PathScore<TaskId> for Coordinated<R> {
    /// The timed route's score, or `None` when two tasks of `path` start outside the timing
    /// limits between them.
    fn path_score(&self, path: &[TaskId]) -> Option<u64> {
        if !self.links(path, None) {
            return self.timed.path_score(path);
        }

        let visits = self.timed.schedule(path)?;
        let agree = visits.iter().enumerate().all(|(index, visit)| {
            visits[index + 1..].iter().all(|other| {
                self.constraints
                    .starts_agree(visit.task, visit.start, other.task, other.start)
            })
        });

        agree.then(|| {
            visits
                .iter()
                .fold(0, |total: u64, visit| total.saturating_add(visit.reward))
        })
    }

    /// The timed route's scores, without building each candidate path, unless `task`
    /// shares an activity with a task of `path`.
    fn insertion_scores(
        &self,
        path: &[TaskId],
        task: &TaskId,
        each: &mut dyn FnMut(usize, Option<u64>),
    ) {
        if !self.links(path, Some(*task)) {
            return self.timed.insertion_scores(path, task, each);
        }

        let mut candidate: Vec<TaskId> = path.to_vec();
        candidate.push(*task);
        for position in (0..=path.len()).rev() {
            if position < path.len() {
                candidate.swap(position, position + 1);
            }

            each(position, self.path_score(&candidate));
        }
    }
}

/// One CCBBA agent with temporal constraints, scheduling its bundle on a route.
///
/// # Examples
///
/// ```
/// use task_allocation::ccbba::{Constraints, Optimism, TimedAgent};
/// use task_allocation::route::{Route, rewards};
/// use task_allocation::sim::{Network, Simulation};
/// use task_allocation::{AgentId, TaskId};
///
/// /// An agent on a line at unit speed; both tasks are at position 10 and take 5.
/// struct Line(u64);
///
/// impl Route for Line {
///     type Place = u64;
///     fn origin(&self) -> u64 { self.0 }
///     fn place(&self, _task: TaskId) -> u64 { 10 }
///     fn travel(&self, from: &u64, to: &u64) -> Option<u64> { Some(from.abs_diff(*to)) }
///     fn duration(&self, _task: TaskId) -> u64 { 5 }
///     fn reward(&self, _task: TaskId, finish: u64) -> Option<u64> {
///         rewards::deadline(10, finish, 100)
///     }
/// }
///
/// // Task 1 needs task 0, and must start 3 to 8 after it.
/// let mut constraints = Constraints::new(2);
/// constraints
///     .add_timed_activity(
///         &[TaskId(0), TaskId(1)],
///         &[&[0, 1], &[0, 0]],
///         &[&[Some(0), Some(-3)], &[Some(8), Some(0)]],
///     )
///     .unwrap();
/// let optimism = Optimism { timeout: 5, solo_attempts: 2, any_attempts: 4 };
///
/// let agents = [Line(0), Line(12)]
///     .into_iter()
///     .enumerate()
///     .map(|(id, line)| TimedAgent::new(AgentId(id), 2, 2, 1, line, constraints.clone(), optimism))
///     .collect();
/// let mut simulation = Simulation::new(agents, Network::default());
///
/// simulation.run_until_settled(100).expect("settles");
/// let starts: Vec<_> = (0..2).map(|task| simulation.agents()[0].start(TaskId(task)).unwrap()).collect();
/// assert!(starts[1] >= starts[0] + 3 && starts[1] <= starts[0] + 8);
/// ```
#[derive(Clone, Debug)]
pub struct TimedAgent<R> {
    /// The CCBBA agent, bidding with the windowed route.
    agent: Agent<Scheduled<R>>,

    /// The conflict table's action for each task of the last message, reused across
    /// messages so receiving does not allocate.
    actions: Vec<(TaskId, Action)>,
}

impl<R: Route> TimedAgent<R> {
    /// Agent `id` of a team of `agents`, scheduling tasks `0..tasks` on `route` under
    /// `constraints`, and holding at most `capacity` of them.
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
        route: R,
        constraints: Constraints,
        optimism: Optimism,
    ) -> Self {
        let windowed = Windowed {
            route,
            windows: RefCell::new(vec![(0, None); tasks]),
        };
        let valuation = BestInsertion(Coordinated {
            timed: Timed(windowed),
            constraints: constraints.clone(),
        });

        Self {
            agent: Agent::new(
                id,
                agents,
                tasks,
                capacity,
                valuation,
                constraints,
                optimism,
            ),
            actions: Vec::with_capacity(tasks),
        }
    }

    /// One round: gives up tasks whose constraints fail, timing included, runs the bundle
    /// phase within the partners' windows, and records the starts of the tasks it holds.
    /// Returns the number of tasks added.
    pub fn build_bundle(&mut self) -> usize {
        self.refresh_windows();
        let added = self.agent.build_bundle();
        self.record_own_starts();

        added
    }

    /// The message this agent broadcasts now.
    #[must_use]
    pub fn message(&mut self) -> TimedMessage {
        TimedMessage {
            claims: self.agent.message(),
            starts: self.view_starts(),
        }
    }

    /// The consensus phase for one message: CBBA's, adopting the sender's start for every
    /// task whose claim it takes. Returns whether anything changed.
    pub fn receive(&mut self, message: &TimedMessage) -> bool {
        let actions = &mut self.actions;
        actions.clear();
        let Some(changed) = self
            .agent
            .inner
            .receive_with_actions(&message.claims, |task, action| actions.push((task, action)))
        else {
            return false;
        };

        {
            let gate = self.agent.inner.valuation();
            let mut view = gate.view.borrow_mut();
            for &(task, action) in &self.actions {
                let start = &mut view.starts[task.index()];
                match action {
                    Action::Update => *start = message.starts.get(task.index()).copied().flatten(),
                    Action::Reset => *start = None,
                    Action::Leave => {}
                }
            }
        }

        self.agent.refresh_view();
        self.forget_unassigned_starts();

        changed
    }

    /// Whether the next round would change anything: what [`Agent::can_bid`] checks, or the
    /// starts of the tasks this agent holds would move.
    #[must_use]
    pub fn can_bid(&self) -> bool {
        self.refresh_windows();

        self.agent.can_bid() || self.own_starts_would_move()
    }

    /// Computes every task's window from its opening and the starts of the tasks it depends
    /// on, as this agent knows them, and hands them to the route.
    fn refresh_windows(&self) {
        let gate = self.agent.inner.valuation();
        let view = gate.view.borrow();
        let constraints = &gate.constraints;
        let claims = self.agent.inner.claims();
        let mut windows = gate.inner.0.timed.0.windows.borrow_mut();

        for (index, window) in windows.iter_mut().enumerate() {
            let task = TaskId(index);
            let mut earliest = i128::from(constraints.opening(task));
            let mut latest: Option<i128> = None;
            for other in constraints.requirements(task) {
                let (Some(_), Some(start)) =
                    (claims[other.index()].winner, view.starts[other.index()])
                else {
                    continue;
                };

                let (from, until) = constraints.window(task, other, start);
                earliest = earliest.max(from.unwrap_or(0));
                latest = match (latest, until) {
                    (Some(current), Some(until)) => Some(current.min(until)),
                    (current, until) => current.or(until),
                };
            }

            *window = clamp_window(earliest, latest);
        }
    }

    /// The visits of this agent's bundle on its route, within the partners' windows, or,
    /// when the windows make the bundle infeasible, without them; `None` when even the route
    /// alone cannot execute it.
    fn own_visits(&self) -> Option<Vec<Visit>> {
        let timed = &self.agent.inner.valuation().inner.0.timed;
        let path = self.agent.bundle().path();

        timed
            .schedule(path)
            .or_else(|| Timed(&timed.0.route).schedule(path))
    }

    /// Records the start of every task this agent holds, and forgets starts of tasks
    /// without a winner.
    fn record_own_starts(&mut self) {
        if let Some(visits) = self.own_visits() {
            let gate = self.agent.inner.valuation();
            let mut view = gate.view.borrow_mut();
            for visit in visits {
                view.starts[visit.task.index()] = Some(visit.start);
            }
        }

        self.forget_unassigned_starts();
    }

    /// Whether recomputing the starts of the tasks this agent holds would change them.
    fn own_starts_would_move(&self) -> bool {
        let Some(visits) = self.own_visits() else {
            return false;
        };

        let view = self.agent.inner.valuation().view.borrow();

        visits
            .iter()
            .any(|visit| view.starts[visit.task.index()] != Some(visit.start))
    }

    /// Forgets the start of every task without a known winner.
    fn forget_unassigned_starts(&self) {
        let gate = self.agent.inner.valuation();
        let claims = self.agent.inner.claims();
        let mut view = gate.view.borrow_mut();
        for (start, claim) in view.starts.iter_mut().zip(claims) {
            if claim.winner.is_none() {
                *start = None;
            }
        }
    }

    /// The start of every task's winning bid, as this agent knows it.
    fn view_starts(&self) -> Vec<Option<u64>> {
        self.agent.inner.valuation().view.borrow().starts.clone()
    }
}

/// A window from bounds that may be negative: the earliest start at least zero, and a
/// latest start before zero made infeasible.
fn clamp_window(earliest: i128, latest: Option<i128>) -> (u64, Option<u64>) {
    let earliest = u64::try_from(earliest.max(0)).unwrap_or(u64::MAX);
    match latest {
        None => (earliest, None),
        Some(latest) if latest < 0 => (u64::MAX, Some(0)),
        Some(latest) => (earliest, Some(u64::try_from(latest).unwrap_or(u64::MAX))),
    }
}

impl<R> TimedAgent<R> {
    /// This agent's identifier.
    #[must_use]
    pub const fn id(&self) -> AgentId {
        self.agent.id()
    }

    /// This agent's route.
    #[must_use]
    pub const fn route(&self) -> &R {
        &self.agent.valuation().0.timed.0.route
    }

    /// The constraints this agent bids under.
    #[must_use]
    pub const fn constraints(&self) -> &Constraints {
        self.agent.constraints()
    }

    /// The tasks this agent holds, with the bid it placed on each.
    #[must_use]
    pub const fn bundle(&self) -> &Bundle<TaskId> {
        self.agent.bundle()
    }

    /// The winner this agent believes in for `task`; `None` when it knows of no bid.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    #[must_use]
    pub fn winner(&self, task: TaskId) -> Option<AgentId> {
        self.agent.winner(task)
    }

    /// The start of `task`'s winning bid, as this agent knows it.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    #[must_use]
    pub fn start(&self, task: TaskId) -> Option<u64> {
        self.agent.inner.valuation().view.borrow().starts[task.index()]
    }
}

impl<R: Route> Participant for TimedAgent<R> {
    type Message = TimedMessage;

    fn id(&self) -> AgentId {
        self.agent.id()
    }

    fn bundle(&self) -> &Bundle<TaskId> {
        self.agent.bundle()
    }

    /// Runs a round and broadcasts the full state, starts included.
    fn act(&mut self, _now: u64, outbox: &mut Vec<TimedMessage>) {
        self.build_bundle();
        outbox.push(self.message());
    }

    /// Runs the consensus phase for `message`.
    fn receive(
        &mut self,
        _now: u64,
        message: &TimedMessage,
        _outbox: &mut Vec<TimedMessage>,
    ) -> bool {
        TimedAgent::receive(self, message)
    }

    /// CBBA's message, then every task's start, each a tag and a time.
    fn message_bytes(message: &TimedMessage) -> u64 {
        let starts = u64::try_from(message.starts.len()).unwrap_or(u64::MAX);

        message.claims.bytes() + 4 + 9 * starts
    }

    /// CCBBA's settled check, plus agreement on every start, among agents and in flight.
    fn is_settled(team: &[Self], in_flight: &[&TimedMessage]) -> bool {
        let Some(first) = team.first() else {
            return true;
        };

        let claims = first.agent.inner.claims();
        let starts = first.view_starts();

        team.iter().all(|agent| agent.agent.inner.is_consistent())
            && !team.iter().any(TimedAgent::can_bid)
            && team
                .iter()
                .all(|agent| agent.agent.inner.claims() == claims && agent.view_starts() == starts)
            && in_flight
                .iter()
                .all(|message| message.claims.claims() == claims && message.starts == starts)
    }
}

/// Unit tests for [`TimedAgent`].
#[cfg(test)]
mod tests {
    use alloc::vec;
    use core::cell::RefCell;

    use super::{Constraints, Coordinated, Optimism, TimedAgent, Windowed};
    use crate::id::{AgentId, TaskId};
    use crate::route::{Route, Timed};
    use crate::valuation::PathScore;

    /// One task at position 5 on a line, worth 1 by time 100.
    struct Point;

    impl Route for Point {
        type Place = u64;

        fn origin(&self) -> u64 {
            0
        }

        fn place(&self, _task: TaskId) -> u64 {
            5
        }

        fn travel(&self, from: &u64, to: &u64) -> Option<u64> {
            Some(from.abs_diff(*to))
        }

        fn reward(&self, _task: TaskId, finish: u64) -> Option<u64> {
            (finish <= 100).then_some(1)
        }
    }

    /// An agent reports what it holds and when, and announces it.
    #[test]
    fn reports_and_announces_its_starts() {
        let optimism = Optimism {
            timeout: 1,
            solo_attempts: 1,
            any_attempts: 1,
        };
        let mut agent = TimedAgent::new(AgentId(0), 1, 1, 1, Point, Constraints::new(1), optimism);
        agent.build_bundle();
        let message = agent.message();

        assert_eq!(agent.id(), AgentId(0));
        assert_eq!(agent.constraints(), &Constraints::new(1));
        assert_eq!(agent.route().origin(), 0);
        assert_eq!(agent.start(TaskId(0)), Some(5));
        assert_eq!(message.start(TaskId(0)), Some(5));
        assert_eq!(message.claims().winner(TaskId(0)), Some(AgentId(0)));
        assert!(!agent.can_bid());
    }

    /// A task never starts before the opening of its window.
    #[test]
    fn openings_hold_back_the_start() {
        let optimism = Optimism {
            timeout: 1,
            solo_attempts: 1,
            any_attempts: 1,
        };
        let mut constraints = Constraints::new(1);
        constraints.set_opening(TaskId(0), 20);
        let mut agent = TimedAgent::new(AgentId(0), 1, 1, 1, Point, constraints, optimism);
        agent.build_bundle();

        assert_eq!(agent.start(TaskId(0)), Some(20));
    }

    /// [`Point`]'s task, taking 4 to do.
    struct Busy;

    impl Route for Busy {
        type Place = u64;

        fn origin(&self) -> u64 {
            Point.origin()
        }

        fn place(&self, task: TaskId) -> u64 {
            Point.place(task)
        }

        fn travel(&self, from: &u64, to: &u64) -> Option<u64> {
            Point.travel(from, to)
        }

        fn duration(&self, _task: TaskId) -> u64 {
            4
        }

        fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
            Point.reward(task, finish)
        }
    }

    /// One agent cannot do two tasks that must start together, since it does them one
    /// after the other; either alone, or both with room between them, is fine.
    #[test]
    fn own_tasks_keep_their_timing_limits() {
        let coordinated = |limit: i64| {
            let mut constraints = Constraints::new(2);
            constraints
                .add_timed_activity(
                    &[TaskId(0), TaskId(1)],
                    &[&[0, 1], &[1, 0]],
                    &[&[Some(0), Some(limit)], &[Some(limit), Some(0)]],
                )
                .expect("a well-formed pair");

            Coordinated {
                timed: Timed(Windowed {
                    route: Busy,
                    windows: RefCell::new(vec![(0, None); 2]),
                }),
                constraints,
            }
        };

        let together = coordinated(0);
        assert_eq!(together.path_score(&[TaskId(0)]), Some(1));
        assert_eq!(together.path_score(&[TaskId(0), TaskId(1)]), None);
        assert_eq!(coordinated(4).path_score(&[TaskId(0), TaskId(1)]), Some(2));
    }
}
