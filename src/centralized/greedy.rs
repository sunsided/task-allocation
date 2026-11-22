//! Sequential greedy assignment: one task at a time, to the agent that values it most.

use crate::assignment::Assignment;
use crate::id::{AgentId, TaskId};
use crate::valuation::{Insertion, Valuation};

/// The sequential greedy assignment of tasks `0..tasks` to `agents`, each holding at most
/// `capacity` tasks.
///
/// Each round considers every agent with room left and every unassigned task, and gives the
/// task with the highest marginal gain to its agent, at its best insertion. Ties go to the
/// smaller agent, then to the smaller task. Tasks with gain zero are never assigned. The
/// result is conflict-free.
///
/// Each round asks every agent about every unassigned task:
/// `O(tasks² × agents)` calls to [`Valuation::best_insertion`] overall.
///
/// # Examples
///
/// ```
/// use task_allocation::{AgentId, TaskId, Valuation, centralized::sequential_greedy};
///
/// /// Values every task at `reward`, for at most one task.
/// struct Single { reward: [u64; 2] }
///
/// impl Valuation<TaskId> for Single {
///     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
///         path.is_empty().then_some(self.reward[task.index()])
///     }
/// }
///
/// let agents = [Single { reward: [5, 4] }, Single { reward: [6, 1] }];
/// let assignment = sequential_greedy(&agents, 2, 1);
///
/// assert_eq!(assignment.holder(TaskId(0)), Some(AgentId(1)));
/// assert_eq!(assignment.holder(TaskId(1)), Some(AgentId(0)));
/// assert_eq!(assignment.value(), 10);
/// ```
#[must_use]
pub fn sequential_greedy<V>(agents: &[V], tasks: usize, capacity: usize) -> Assignment
where
    V: Valuation<TaskId>,
{
    let mut assignment = Assignment::empty(agents.len());

    while let Some((agent, task, insertion)) = best_remaining(agents, &assignment, tasks, capacity)
    {
        assignment.bundle_mut(agent).insert(task, insertion);
    }

    assignment
}

/// The most valuable assignment of one more task: the agent, the task and its insertion;
/// `None` when no agent with room left gains from any unassigned task.
fn best_remaining<V>(
    agents: &[V],
    assignment: &Assignment,
    tasks: usize,
    capacity: usize,
) -> Option<(AgentId, TaskId, Insertion)>
where
    V: Valuation<TaskId>,
{
    let mut best: Option<(AgentId, TaskId, Insertion)> = None;
    for (index, valuation) in agents.iter().enumerate() {
        let agent = AgentId(index);
        let bundle = assignment.bundle(agent);
        if bundle.len() >= capacity {
            continue;
        }

        let unassigned = (0..tasks)
            .map(TaskId)
            .filter(|&task| assignment.holder(task).is_none());
        for task in unassigned {
            let Some(insertion) = valuation.best_insertion(bundle.path(), &task) else {
                continue;
            };

            let floor = best.map_or(0, |(_, _, current)| current.gain);
            if insertion.gain > floor {
                best = Some((agent, task, insertion));
            }
        }
    }

    best
}
