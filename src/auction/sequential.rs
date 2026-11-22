//! Sequential single-item auctions.

use crate::assignment::Assignment;
use crate::id::{AgentId, TaskId};
use crate::valuation::{Insertion, Valuation};

/// Auctions the tasks one at a time, in the order `order` lists them, among `agents` that
/// hold at most `capacity` tasks each.
///
/// For each task, every agent with room left bids its marginal gain on its current path,
/// and the highest positive bid wins; ties go to the smaller agent. The winner inserts the
/// task at its best position. A task nobody bids on stays unassigned. Tasks listed twice
/// are auctioned once; identifiers of `tasks` or more are ignored.
///
/// Each task asks every agent once: `order.len() × agents` calls to
/// [`Valuation::best_insertion`].
///
/// # Examples
///
/// ```
/// use task_allocation::{AgentId, TaskId, Valuation, auction::sequential};
///
/// /// Values each task at a fixed reward, for at most one task.
/// struct Single([u64; 2]);
///
/// impl Valuation<TaskId> for Single {
///     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
///         path.is_empty().then_some(self.0[task.index()])
///     }
/// }
///
/// let agents = [Single([5, 0]), Single([6, 5])];
///
/// // Auctioned first, task 0 goes to agent 1, which then has no room for task 1.
/// let first = sequential(&agents, 2, 1, [TaskId(0), TaskId(1)]);
/// assert_eq!(first.value(), 6);
///
/// // The other way round, both tasks find a taker.
/// let second = sequential(&agents, 2, 1, [TaskId(1), TaskId(0)]);
/// assert_eq!(second.holder(TaskId(0)), Some(AgentId(0)));
/// assert_eq!(second.value(), 10);
/// ```
#[must_use]
pub fn sequential<V, I>(agents: &[V], tasks: usize, capacity: usize, order: I) -> Assignment
where
    V: Valuation<TaskId>,
    I: IntoIterator<Item = TaskId>,
{
    let mut assignment = Assignment::empty(agents.len());

    for task in order {
        let unauctioned = task.index() < tasks && assignment.holder(task).is_none();
        if !unauctioned {
            continue;
        }

        if let Some((winner, insertion)) = highest_bid(agents, &assignment, capacity, task) {
            assignment.bundle_mut(winner).insert(task, insertion);
        }
    }

    assignment
}

/// The agent with the highest positive bid on `task`, among those with room left, with
/// its insertion; ties go to the smaller agent.
fn highest_bid<V>(
    agents: &[V],
    assignment: &Assignment,
    capacity: usize,
    task: TaskId,
) -> Option<(AgentId, Insertion)>
where
    V: Valuation<TaskId>,
{
    let mut best: Option<(AgentId, Insertion)> = None;
    for (index, valuation) in agents.iter().enumerate() {
        let bundle = assignment.bundle(AgentId(index));
        if bundle.len() >= capacity {
            continue;
        }

        let Some(insertion) = valuation.best_insertion(bundle.path(), &task) else {
            continue;
        };

        let floor = best.map_or(0, |(_, current)| current.gain);
        if insertion.gain > floor {
            best = Some((AgentId(index), insertion));
        }
    }

    best
}
