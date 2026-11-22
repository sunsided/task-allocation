//! Timed routes: a ready-made path score for agents that travel between task sites.
//!
//! An agent leaves its origin at its departure time, travels to each task's place in path
//! order, waits for the task's earliest start if it arrives early, works for the task's
//! duration, and collects a reward that depends on when it finishes. Implement [`Route`] for
//! the travel times, windows and rewards, and wrap it in [`Timed`] to get a [`PathScore`];
//! wrap that in [`BestInsertion`](crate::BestInsertion) to get a [`Valuation`](crate::Valuation)
//! that inserts each task where it adds the most.
//!
//! ```mermaid
//! flowchart LR
//!     origin --> travel[travel to the task's place]
//!     travel --> wait[wait for the earliest start]
//!     wait --> work[work for the duration]
//!     work --> reward[reward for the finish time]
//!     reward -->|next task| travel
//! ```
//!
//! Whether gains diminish depends on the route: rewards that never grow with later finish
//! times, and travel times that obey the triangle inequality, keep feasibility monotone, but
//! decaying rewards can still let one task raise another's gain: once the path passes near a
//! site, visiting it delays the rest less. In the plane, [`rewards::halving`] shows this
//! within a handful of sites.
//! Run [`check_diminishing`](crate::check_diminishing) on representative instances.
//!
//! [`rewards`] has common reward shapes.

pub mod rewards;

use alloc::vec::Vec;

use crate::id::TaskId;
use crate::valuation::PathScore;

/// How an agent travels between task sites, and what each task is worth when.
///
/// Times are in a unit of the implementer's choice, the same everywhere: ticks, seconds or
/// minutes.
pub trait Route {
    /// Where the agent can be: a node of a graph, a grid cell, a point.
    type Place;

    /// Where the agent starts.
    fn origin(&self) -> Self::Place;

    /// When the agent leaves its origin. The default is time zero.
    fn departure(&self) -> u64 {
        0
    }

    /// Where `task` is done.
    fn place(&self, task: TaskId) -> Self::Place;

    /// How long the agent takes from `from` to `to`; `None` when it cannot get there.
    fn travel(&self, from: &Self::Place, to: &Self::Place) -> Option<u64>;

    /// How long `task` takes once started. The default is zero.
    fn duration(&self, task: TaskId) -> u64 {
        let _ = task;

        0
    }

    /// The earliest time `task` may start; an agent that arrives earlier waits. The default
    /// is time zero.
    fn earliest_start(&self, task: TaskId) -> u64 {
        let _ = task;

        0
    }

    /// What `task` is worth when finished at `finish`; `None` when that is too late. See
    /// [`rewards`] for common shapes.
    fn reward(&self, task: TaskId, finish: u64) -> Option<u64>;
}

impl<R: Route + ?Sized> Route for &R {
    type Place = R::Place;

    fn origin(&self) -> Self::Place {
        (**self).origin()
    }

    fn departure(&self) -> u64 {
        (**self).departure()
    }

    fn place(&self, task: TaskId) -> Self::Place {
        (**self).place(task)
    }

    fn travel(&self, from: &Self::Place, to: &Self::Place) -> Option<u64> {
        (**self).travel(from, to)
    }

    fn duration(&self, task: TaskId) -> u64 {
        (**self).duration(task)
    }

    fn earliest_start(&self, task: TaskId) -> u64 {
        (**self).earliest_start(task)
    }

    fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
        (**self).reward(task, finish)
    }
}

/// One task of a timed route: when the agent arrives, starts, finishes, and what it earns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Visit {
    /// The task.
    pub task: TaskId,

    /// When the agent arrives at the task's place.
    pub arrival: u64,

    /// When the agent starts the task: its arrival, or the task's earliest start if later.
    pub start: u64,

    /// When the agent finishes the task.
    pub finish: u64,

    /// What the task earns at that finish time.
    pub reward: u64,
}

/// A [`PathScore`] from a [`Route`]: the sum of the rewards along the path, or `None` when a
/// place is unreachable or a task finishes too late.
///
/// # Examples
///
/// ```
/// use task_allocation::route::{Route, Timed, rewards};
/// use task_allocation::{BestInsertion, PathScore, TaskId, Valuation};
///
/// /// A rover on a line, at one meter per second, with three sites.
/// struct Line;
///
/// const SITES: [u64; 3] = [10, 40, 90];
///
/// impl Route for Line {
///     type Place = u64;
///
///     fn origin(&self) -> u64 { 0 }
///     fn place(&self, task: TaskId) -> u64 { SITES[task.index()] }
///     fn travel(&self, from: &u64, to: &u64) -> Option<u64> { Some(from.abs_diff(*to)) }
///     fn duration(&self, _task: TaskId) -> u64 { 5 }
///
///     /// Each site is worth 100 if done within 60 seconds.
///     fn reward(&self, _task: TaskId, finish: u64) -> Option<u64> {
///         rewards::deadline(100, finish, 60)
///     }
/// }
///
/// let route = Timed(Line);
/// let visits = route.schedule(&[TaskId(0), TaskId(1)]).unwrap();
/// assert_eq!((visits[1].arrival, visits[1].finish), (45, 50));
///
/// // The third site is too far to make the deadline.
/// assert_eq!(route.path_score(&[TaskId(2)]), None);
///
/// // Visiting the second site after the first still makes its deadline.
/// let valuation = BestInsertion(Timed(Line));
/// assert_eq!(valuation.marginal_score(&[TaskId(0)], &TaskId(1)), Some(100));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Timed<R>(pub R);

impl<R: Route> Timed<R> {
    /// The visits of `path`, in order; `None` when a place is unreachable or a task finishes
    /// too late. Times saturate at [`u64::MAX`].
    #[must_use]
    pub fn schedule(&self, path: &[TaskId]) -> Option<Vec<Visit>> {
        let mut visits = Vec::with_capacity(path.len());
        self.walk(path, |visit| visits.push(visit))?;

        Some(visits)
    }

    /// Walks `path`, calling `visit` for each task in order; returns the total reward, or
    /// `None` as soon as a place is unreachable or a task finishes too late.
    fn walk(&self, path: &[TaskId], mut visit: impl FnMut(Visit)) -> Option<u64> {
        let route = &self.0;
        let mut at = route.origin();
        let mut clock = route.departure();
        let mut total: u64 = 0;

        for &task in path {
            let place = route.place(task);
            let arrival = clock.saturating_add(route.travel(&at, &place)?);
            let start = arrival.max(route.earliest_start(task));
            let finish = start.saturating_add(route.duration(task));
            let reward = route.reward(task, finish)?;

            visit(Visit {
                task,
                arrival,
                start,
                finish,
                reward,
            });
            total = total.saturating_add(reward);
            at = place;
            clock = finish;
        }

        Some(total)
    }
}

impl<R: Route> PathScore<TaskId> for Timed<R> {
    fn path_score(&self, path: &[TaskId]) -> Option<u64> {
        self.walk(path, |_| {})
    }
}

/// Unit tests for timed routes.
#[cfg(test)]
mod tests;
