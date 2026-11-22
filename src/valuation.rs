//! How agents value tasks: marginal gains given the tasks they already hold.
//!
//! An agent's *path* is its tasks in the order it will execute them. A [`Valuation`] scores
//! what one more task adds to that path. Allocation algorithms build bundles by repeatedly
//! adding the task with the highest marginal gain, so the gains of a bundle add up to its
//! value:
//!
//! ```text
//! value(path) = sum of the gains recorded while the path was built
//! ```
//!
//! Two ways to write a valuation:
//!
//! - implement [`Valuation::marginal_score`] directly, when a new task always goes at the
//!   end of the path (or when the valuation decides on its own where it goes and only the
//!   gain matters to the caller);
//! - implement [`PathScore`] for a whole path and wrap it in [`BestInsertion`], which tries
//!   every insertion position and keeps the best.
//!
//! Consensus algorithms such as CBBA come with performance guarantees only when gains
//! *diminish*: a task never becomes more valuable to an agent because the agent took on more
//! work. [`check_diminishing`] tests that on a small task set.

mod diminishing;

pub use diminishing::{Violation, check_diminishing};

use alloc::vec::Vec;

/// An agent's valuation of tasks, used to place bids.
///
/// # Contract
///
/// - Higher scores are better. A score is a marginal gain: how much the agent's value grows
///   when it adds `task` to `path`.
/// - `None` means the agent cannot take the task on top of `path`, for example because it
///   would miss a deadline or exceed a capacity. An agent places no bid on such a task.
/// - [`best_insertion`](Valuation::best_insertion) and
///   [`marginal_score`](Valuation::marginal_score) agree: the insertion's gain is the
///   marginal score.
/// - Valuations are deterministic: the same path and task always give the same score.
///   Agents that disagree on scores cannot agree on winners.
///
/// # Examples
///
/// A rover that collects a fixed reward for each task it finishes before the task's
/// deadline, working through its path in order:
///
/// ```
/// use task_allocation::Valuation;
///
/// struct Task { duration: u64, deadline: u64, reward: u64 }
/// struct Rover;
///
/// impl Valuation<Task> for Rover {
///     fn marginal_score(&self, path: &[Task], task: &Task) -> Option<u64> {
///         let busy_until: u64 = path.iter().map(|held| held.duration).sum();
///
///         (busy_until + task.duration <= task.deadline).then_some(task.reward)
///     }
/// }
///
/// let late = Task { duration: 50, deadline: 60, reward: 4 };
/// let early = Task { duration: 20, deadline: 40, reward: 9 };
///
/// assert_eq!(Rover.marginal_score(&[], &early), Some(9));
/// assert_eq!(Rover.marginal_score(&[late], &early), None);
/// ```
pub trait Valuation<Task> {
    /// The score of adding `task` to the agent's current bundle; higher is better, and
    /// `None` means the agent cannot take the task.
    ///
    /// `bundle` is the agent's path: the tasks it holds, in execution order.
    fn marginal_score(&self, bundle: &[Task], task: &Task) -> Option<u64>;

    /// Where `task` goes in `path`, and what it adds there; `None` when the agent cannot
    /// take it.
    ///
    /// The default appends the task to the end of the path and scores it with
    /// [`marginal_score`](Valuation::marginal_score). Override it when the position matters,
    /// or use [`BestInsertion`].
    fn best_insertion(&self, path: &[Task], task: &Task) -> Option<Insertion> {
        let gain = self.marginal_score(path, task)?;

        Some(Insertion {
            position: path.len(),
            gain,
        })
    }
}

impl<Task, V: Valuation<Task> + ?Sized> Valuation<Task> for &V {
    fn marginal_score(&self, bundle: &[Task], task: &Task) -> Option<u64> {
        (**self).marginal_score(bundle, task)
    }

    fn best_insertion(&self, path: &[Task], task: &Task) -> Option<Insertion> {
        (**self).best_insertion(path, task)
    }
}

/// Where a task goes in an agent's path, and what it adds there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Insertion {
    /// Index in the path at which the task is inserted; at most the path's length, which
    /// appends it. Tasks at this index and after shift back by one.
    pub position: usize,

    /// The marginal gain of the insertion, as [`Valuation::marginal_score`] reports it.
    pub gain: u64,
}

/// The value of a whole path: the tasks an agent holds, in execution order.
///
/// Wrap a path score in [`BestInsertion`] to get a [`Valuation`] that inserts each new task
/// where it adds the most.
pub trait PathScore<Task> {
    /// The value of executing `path` in order; `None` when the agent cannot execute it,
    /// for example because a task would miss its deadline.
    ///
    /// The empty path must be feasible.
    fn path_score(&self, path: &[Task]) -> Option<u64>;
}

/// A [`Valuation`] that tries every insertion position of a [`PathScore`] and keeps the best.
///
/// The marginal gain is the difference between the path's value with and without the new
/// task. A task that lowers the path's value scores zero, which never wins a bid. Of equally
/// good positions the earliest wins.
///
/// Each call scores `path.len() + 1` candidate paths and allocates one scratch path.
///
/// # Examples
///
/// ```
/// use task_allocation::{BestInsertion, Insertion, PathScore, Valuation};
///
/// /// Visits points on a line, starting at 0: 50 per point, minus the distance traveled.
/// struct Walker;
///
/// impl PathScore<i64> for Walker {
///     fn path_score(&self, path: &[i64]) -> Option<u64> {
///         let mut at: i64 = 0;
///         let mut traveled = 0;
///
///         for &point in path {
///             traveled += at.abs_diff(point);
///             at = point;
///         }
///
///         (50 * path.len() as u64).checked_sub(traveled)
///     }
/// }
///
/// let walker = BestInsertion(Walker);
///
/// // Visiting 5 on the way to 10 adds no travel, so it gains the full 50.
/// assert_eq!(
///     walker.best_insertion(&[10], &5),
///     Some(Insertion { position: 0, gain: 50 })
/// );
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BestInsertion<S>(pub S);

impl<Task: Clone, S: PathScore<Task>> Valuation<Task> for BestInsertion<S> {
    fn marginal_score(&self, bundle: &[Task], task: &Task) -> Option<u64> {
        self.best_insertion(bundle, task)
            .map(|insertion| insertion.gain)
    }

    fn best_insertion(&self, path: &[Task], task: &Task) -> Option<Insertion> {
        let base = self.0.path_score(path)?;
        let mut candidate: Vec<Task> = path.to_vec();
        candidate.push(task.clone());

        let mut best: Option<Insertion> = None;
        for position in (0..=path.len()).rev() {
            if position < path.len() {
                candidate.swap(position, position + 1);
            }

            best = better_insertion(best, self.score_candidate(&candidate, base, position));
        }

        best
    }
}

impl<S> BestInsertion<S> {
    /// Scores `candidate`, the path with the new task at `position`, against the old path's
    /// value `base`; `None` when the candidate is infeasible.
    fn score_candidate<Task>(
        &self,
        candidate: &[Task],
        base: u64,
        position: usize,
    ) -> Option<Insertion>
    where
        S: PathScore<Task>,
    {
        let value = self.0.path_score(candidate)?;

        Some(Insertion {
            position,
            gain: value.saturating_sub(base),
        })
    }
}

/// The better of two insertions: the higher gain, then the earlier position.
fn better_insertion(current: Option<Insertion>, candidate: Option<Insertion>) -> Option<Insertion> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => {
            let candidate_wins = candidate.gain > current.gain
                || (candidate.gain == current.gain && candidate.position < current.position);

            Some(if candidate_wins { candidate } else { current })
        }
        (current, None) => current,
        (None, candidate) => candidate,
    }
}

/// Unit tests for the valuation traits.
#[cfg(test)]
mod tests;
