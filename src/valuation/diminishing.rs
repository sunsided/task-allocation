//! Brute-force check that a valuation's marginal gains diminish.
//!
//! A valuation has *diminishing marginal gains* (DMG) when adding work never makes another
//! task more valuable: for every path `p`, every task `k` added to it and every other task
//! `j`, the gain of `j` after `p + k` is at most its gain after `p`. A task that was
//! infeasible stays infeasible. CBBA relies on this to converge to the sequential greedy
//! assignment, which scores at least half the optimum.

use alloc::vec::Vec;
use core::fmt;

use super::Valuation;

/// A counterexample to diminishing marginal gains: adding `added` to `path` raised the gain
/// of `task`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation<Task> {
    /// The path before `added` was inserted, in execution order.
    pub path: Vec<Task>,

    /// The task whose insertion raised the gain of `task`.
    pub added: Task,

    /// The task whose gain went up.
    pub task: Task,

    /// The gain of `task` on `path`; `None` when it was infeasible.
    pub before: Option<u64>,

    /// The gain of `task` once `added` is in the path; `None` when it is infeasible.
    pub after: Option<u64>,
}

impl<Task: fmt::Debug> fmt::Display for Violation<Task> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "adding {:?} to path {:?} raised the gain of {:?} from {:?} to {:?}",
            self.added, self.path, self.task, self.before, self.after
        )
    }
}

impl<Task: fmt::Debug> core::error::Error for Violation<Task> {}

/// Checks that `valuation` has diminishing marginal gains over `tasks`, for agents that hold
/// at most `capacity` tasks.
///
/// Gains matter only on paths shorter than `capacity`, where the agent can still take a
/// task, so only those are compared.
///
/// Paths are built the way allocation algorithms build them: each task goes where
/// [`Valuation::best_insertion`] puts it. The check explores every build order, so it is
/// meant for small task sets: about `tasks.len()! / (tasks.len() - capacity)!` paths.
///
/// # Errors
///
/// Returns the first [`Violation`] found: a path, a task added to it, and another task whose
/// gain went up (or became feasible) as a result.
///
/// # Examples
///
/// ```
/// use task_allocation::{Valuation, check_diminishing};
///
/// /// Pays for each task, but more once the agent is already busy: not diminishing.
/// struct Eager;
///
/// impl Valuation<u64> for Eager {
///     fn marginal_score(&self, path: &[u64], task: &u64) -> Option<u64> {
///         Some(task + path.len() as u64)
///     }
/// }
///
/// let violation = check_diminishing(&Eager, &[1, 2], 2).unwrap_err();
/// assert_eq!((violation.before, violation.after), (Some(2), Some(3)));
/// ```
pub fn check_diminishing<Task, V>(
    valuation: &V,
    tasks: &[Task],
    capacity: usize,
) -> Result<(), Violation<Task>>
where
    Task: Clone,
    V: Valuation<Task> + ?Sized,
{
    let mut search = Search {
        valuation,
        tasks,
        capacity,
        path: Vec::new(),
        used: alloc::vec![false; tasks.len()],
    };

    search.explore()
}

/// The state of the depth-first search over build orders.
struct Search<'a, Task, V: ?Sized> {
    /// The valuation under test.
    valuation: &'a V,

    /// The tasks paths are built from.
    tasks: &'a [Task],

    /// The longest path to explore.
    capacity: usize,

    /// The current path, in execution order.
    path: Vec<Task>,

    /// Whether each task of `tasks` is on the current path.
    used: Vec<bool>,
}

impl<Task, V> Search<'_, Task, V>
where
    Task: Clone,
    V: Valuation<Task> + ?Sized,
{
    /// Checks every extension of the current path by one task, then recurses into it.
    fn explore(&mut self) -> Result<(), Violation<Task>> {
        if self.path.len() >= self.capacity {
            return Ok(());
        }

        for added in 0..self.tasks.len() {
            if self.used[added] {
                continue;
            }

            let Some(insertion) = self
                .valuation
                .best_insertion(&self.path, &self.tasks[added])
            else {
                continue;
            };

            let mut extended = self.path.clone();
            extended.insert(insertion.position, self.tasks[added].clone());
            if extended.len() < self.capacity {
                self.check_extension(added, &extended)?;
            }

            self.descend(added, extended)?;
        }

        Ok(())
    }

    /// Checks that no other task gained from adding task `added`, which turned the current
    /// path into `extended`.
    fn check_extension(&self, added: usize, extended: &[Task]) -> Result<(), Violation<Task>> {
        for task in 0..self.tasks.len() {
            if task == added || self.used[task] {
                continue;
            }

            let before = self.valuation.marginal_score(&self.path, &self.tasks[task]);
            let after = self.valuation.marginal_score(extended, &self.tasks[task]);
            if after > before {
                return Err(Violation {
                    path: self.path.clone(),
                    added: self.tasks[added].clone(),
                    task: self.tasks[task].clone(),
                    before,
                    after,
                });
            }
        }

        Ok(())
    }

    /// Makes `extended` the current path, explores it, and restores the current path.
    fn descend(&mut self, added: usize, extended: Vec<Task>) -> Result<(), Violation<Task>> {
        let previous = core::mem::replace(&mut self.path, extended);
        self.used[added] = true;

        let result = self.explore();

        self.used[added] = false;
        self.path = previous;

        result
    }
}
