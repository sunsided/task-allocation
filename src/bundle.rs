//! An agent's bundle: the tasks it holds, in the order it took them on, plus their path.
//!
//! The *bundle order* is the order in which the agent added tasks, and each entry records
//! the marginal gain the task had when it was added. The *path* is the same tasks in
//! execution order, as [`Valuation::best_insertion`] placed them.
//!
//! Consensus algorithms need both. Gains are only valid for the tasks added before, so when
//! an agent loses a task it also drops every task it added after it
//! ([`Bundle::release_from`]): their gains assumed the lost task was still there.
//!
//! ```mermaid
//! flowchart LR
//!     candidates[candidate tasks] --> best[best insertion per task]
//!     best --> pick[highest accepted gain]
//!     pick --> insert[insert into path, record gain]
//!     insert -->|below capacity| best
//! ```

use alloc::vec::Vec;

use crate::valuation::{Insertion, Valuation};

/// The tasks an agent holds, in bundle order and in path order.
///
/// # Invariants
///
/// - The bundle and the path hold the same tasks, each at most once.
/// - Gains are those the tasks had when they were added, given the tasks added before them.
///
/// # Examples
///
/// ```
/// use task_allocation::{Bundle, Valuation};
///
/// /// Values task `t` at `t`, but each task costs one more point than the last.
/// struct Busy;
///
/// impl Valuation<u64> for Busy {
///     fn marginal_score(&self, path: &[u64], task: &u64) -> Option<u64> {
///         task.checked_sub(path.len() as u64)
///     }
/// }
///
/// let mut bundle = Bundle::new();
/// let added = bundle.extend_greedily(&Busy, &[3, 9, 5], 2, |_, gain| gain > 0);
///
/// assert_eq!(added, 2);
/// assert_eq!(bundle.tasks().collect::<Vec<_>>(), [&9, &5]);
/// assert_eq!(bundle.value(), 9 + 4);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle<Task> {
    /// The tasks with their gains, in the order they were added.
    added: Vec<(Task, u64)>,

    /// The same tasks in execution order.
    path: Vec<Task>,
}

impl<Task> Default for Bundle<Task> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Task> Bundle<Task> {
    /// An empty bundle.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            added: Vec::new(),
            path: Vec::new(),
        }
    }

    /// The number of tasks held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.added.len()
    }

    /// Whether the bundle holds no task.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
    }

    /// The tasks in execution order.
    #[must_use]
    pub fn path(&self) -> &[Task] {
        &self.path
    }

    /// The tasks in the order they were added.
    pub fn tasks(&self) -> impl ExactSizeIterator<Item = &Task> + '_ {
        self.added.iter().map(|(task, _)| task)
    }

    /// The tasks with the gain each had when it was added, in the order they were added.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = (&Task, u64)> + '_ {
        self.added.iter().map(|(task, gain)| (task, *gain))
    }

    /// The bundle's value: the sum of the recorded gains.
    ///
    /// Saturates at [`u64::MAX`].
    #[must_use]
    pub fn value(&self) -> u64 {
        self.added
            .iter()
            .fold(0, |total, (_, gain)| total.saturating_add(*gain))
    }

    /// Adds `task` at the end of the bundle order and at `insertion.position` in the path,
    /// recording `insertion.gain`.
    ///
    /// The caller keeps the bundle free of duplicates; see [`Bundle::contains`].
    ///
    /// # Panics
    ///
    /// Panics when `insertion.position` is greater than the path's length.
    pub fn insert(&mut self, task: Task, insertion: Insertion)
    where
        Task: Clone,
    {
        assert!(
            insertion.position <= self.path.len(),
            "insertion position {} beyond the path's length {}",
            insertion.position,
            self.path.len()
        );

        self.path.insert(insertion.position, task.clone());
        self.added.push((task, insertion.gain));
    }

    /// Removes every task added at bundle index `index` or later, and returns them in the
    /// order they were added. An `index` at or past the end removes nothing.
    ///
    /// Use it when the agent loses the task at `index`: the gains of later tasks assumed that
    /// task was still held.
    pub fn release_from(&mut self, index: usize) -> Vec<Task>
    where
        Task: PartialEq,
    {
        let mut released = Vec::new();
        self.release_from_with(index, |task| released.push(task));

        released
    }

    /// Removes every task added at bundle index `index` or later, as
    /// [`release_from`](Bundle::release_from) does, handing each to `released` in the order
    /// they were added instead of collecting them. It does not allocate.
    pub fn release_from_with(&mut self, index: usize, mut released: impl FnMut(Task))
    where
        Task: PartialEq,
    {
        if index >= self.added.len() {
            return;
        }

        let dropped = &self.added[index..];
        self.path
            .retain(|task| !dropped.iter().any(|(held, _)| held == task));

        for (task, _) in self.added.drain(index..) {
            released(task);
        }
    }

    /// Removes every task.
    pub fn clear(&mut self) {
        self.added.clear();
        self.path.clear();
    }
}

impl<Task: PartialEq> Bundle<Task> {
    /// Whether the bundle holds `task`.
    #[must_use]
    pub fn contains(&self, task: &Task) -> bool {
        self.path.contains(task)
    }

    /// The bundle index of `task`: how many tasks were added before it; `None` when the
    /// bundle does not hold it.
    #[must_use]
    pub fn index_of(&self, task: &Task) -> Option<usize> {
        self.added.iter().position(|(held, _)| held == task)
    }
}

impl<Task: Clone + PartialEq> Bundle<Task> {
    /// The candidate with the highest gain on the current path that `accept` allows, with
    /// its insertion; `None` when no candidate is feasible and accepted.
    ///
    /// Tasks the bundle already holds are skipped. Of equal gains, the candidate listed first
    /// wins. `accept` is asked about each feasible task, with its gain, that would beat the
    /// best so far; a bidding agent uses it to skip tasks it cannot outbid.
    pub fn best_candidate<'c, V, F>(
        &self,
        valuation: &V,
        candidates: &'c [Task],
        mut accept: F,
    ) -> Option<(&'c Task, Insertion)>
    where
        V: Valuation<Task> + ?Sized,
        F: FnMut(&Task, u64) -> bool,
    {
        let mut best: Option<(&Task, Insertion)> = None;
        for task in candidates {
            if self.contains(task) {
                continue;
            }

            let Some(insertion) = valuation.best_insertion(&self.path, task) else {
                continue;
            };

            let beats_best = best.is_none_or(|(_, current)| insertion.gain > current.gain);
            if beats_best && accept(task, insertion.gain) {
                best = Some((task, insertion));
            }
        }

        best
    }

    /// Adds the best accepted candidate, again and again, until the bundle holds `capacity`
    /// tasks or no candidate is left; returns the number of tasks added.
    ///
    /// This is the greedy bundle construction of CBBA's bundle phase: each round adds the
    /// task with the highest marginal gain on the current path (see
    /// [`best_candidate`](Bundle::best_candidate)) at its best position.
    pub fn extend_greedily<V, F>(
        &mut self,
        valuation: &V,
        candidates: &[Task],
        capacity: usize,
        mut accept: F,
    ) -> usize
    where
        V: Valuation<Task> + ?Sized,
        F: FnMut(&Task, u64) -> bool,
    {
        let before = self.len();
        while self.len() < capacity {
            let Some((task, insertion)) = self.best_candidate(valuation, candidates, &mut accept)
            else {
                break;
            };

            self.insert(task.clone(), insertion);
        }

        self.len() - before
    }
}

/// Unit tests for [`Bundle`].
#[cfg(test)]
mod tests;
