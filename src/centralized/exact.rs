//! The optimal assignment, by exhaustive search over build orders and task partitions.

use alloc::vec;
use alloc::vec::Vec;

use crate::assignment::Assignment;
use crate::bundle::Bundle;
use crate::id::TaskId;
use crate::valuation::Valuation;

/// The most tasks [`optimal`] accepts. The search grows with `3^tasks` per agent.
pub const MAX_OPTIMAL_TASKS: usize = 16;

/// The assignment of tasks `0..tasks` to `agents`, each holding at most `capacity` tasks,
/// with the highest [`Assignment::value`].
///
/// Tasks may stay unassigned. A task set's value for an agent is the best total gain over
/// every order in which the agent can build it, inserting each task where
/// [`Valuation::best_insertion`] puts it. The search tries every build order of every task
/// set for every agent, then every split of the tasks among the agents. Use it as a
/// reference on small instances: up to about eight tasks it finishes in well under a second.
/// Of equally good assignments, the first one found is returned.
///
/// # Panics
///
/// Panics when `tasks` exceeds [`MAX_OPTIMAL_TASKS`].
///
/// # Examples
///
/// ```
/// use task_allocation::{AgentId, TaskId, Valuation, centralized::{optimal, sequential_greedy}};
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
/// let agents = [Single { reward: [5, 0] }, Single { reward: [6, 5] }];
///
/// // Greedy gives task 0 to agent 1, and agent 0 gains nothing from task 1.
/// assert_eq!(sequential_greedy(&agents, 2, 1).value(), 6);
///
/// // The optimum gives task 0 to agent 0 and task 1 to agent 1.
/// let best = optimal(&agents, 2, 1);
/// assert_eq!(best.value(), 10);
/// assert_eq!(best.holder(TaskId(0)), Some(AgentId(0)));
/// ```
#[must_use]
pub fn optimal<V>(agents: &[V], tasks: usize, capacity: usize) -> Assignment
where
    V: Valuation<TaskId>,
{
    assert!(
        tasks <= MAX_OPTIMAL_TASKS,
        "optimal() searches at most {MAX_OPTIMAL_TASKS} tasks, got {tasks}"
    );

    let best_bundles: Vec<Vec<Option<Bundle<TaskId>>>> = agents
        .iter()
        .map(|valuation| best_bundle_per_set(valuation, tasks, capacity))
        .collect();

    let splits = best_splits(&best_bundles, tasks);
    reconstruct(&best_bundles, &splits, tasks)
}

/// For each task set, as a bit mask over tasks, the most valuable bundle the agent can build
/// from exactly that set; `None` for sets it cannot build.
fn best_bundle_per_set<V>(
    valuation: &V,
    tasks: usize,
    capacity: usize,
) -> Vec<Option<Bundle<TaskId>>>
where
    V: Valuation<TaskId>,
{
    let mut best = vec![None; 1 << tasks];
    let mut bundle = Bundle::new();
    record_builds(valuation, tasks, capacity, 0, &mut bundle, &mut best);

    best
}

/// Records `bundle`, built from the tasks in `mask`, if it beats the best bundle for that
/// set, then tries every task that extends it.
fn record_builds<V>(
    valuation: &V,
    tasks: usize,
    capacity: usize,
    mask: usize,
    bundle: &mut Bundle<TaskId>,
    best: &mut [Option<Bundle<TaskId>>],
) where
    V: Valuation<TaskId>,
{
    let beats_best = best[mask]
        .as_ref()
        .is_none_or(|current| bundle.value() > current.value());
    if beats_best {
        best[mask] = Some(bundle.clone());
    }

    if bundle.len() >= capacity {
        return;
    }

    for task in (0..tasks).filter(|task| mask & (1 << task) == 0) {
        let Some(insertion) = valuation.best_insertion(bundle.path(), &TaskId(task)) else {
            continue;
        };

        let index = bundle.len();
        bundle.insert(TaskId(task), insertion);
        record_builds(valuation, tasks, capacity, mask | (1 << task), bundle, best);
        bundle.release_from(index);
    }
}

/// The best split of task sets among agents, built agent by agent.
///
/// `splits[agent][mask]` is the set the agent takes, out of `mask`, in the best assignment of
/// `mask` to agents `0..=agent`, together with that assignment's value; `None` when no
/// assignment of agents `0..=agent` covers exactly `mask`.
fn best_splits(
    best_bundles: &[Vec<Option<Bundle<TaskId>>>],
    tasks: usize,
) -> Vec<Vec<Option<(usize, u64)>>> {
    let sets = 1_usize << tasks;
    let mut previous: Vec<Option<u64>> = vec![None; sets];
    previous[0] = Some(0);

    let mut splits = Vec::with_capacity(best_bundles.len());
    for bundles in best_bundles {
        let layer: Vec<Option<(usize, u64)>> = (0..sets)
            .map(|mask| best_share(bundles, &previous, mask))
            .collect();

        previous = layer
            .iter()
            .map(|entry| entry.map(|(_, value)| value))
            .collect();
        splits.push(layer);
    }

    splits
}

/// The set this agent takes out of `mask`, and the total value, that maximizes the agent's
/// bundle plus the best assignment of the rest to the previous agents.
fn best_share(
    bundles: &[Option<Bundle<TaskId>>],
    previous: &[Option<u64>],
    mask: usize,
) -> Option<(usize, u64)> {
    let mut best: Option<(usize, u64)> = None;
    let mut share = mask;
    loop {
        let candidate = bundles[share]
            .as_ref()
            .zip(previous[mask & !share])
            .map(|(bundle, rest)| (share, rest.saturating_add(bundle.value())));
        let improves =
            candidate.is_some_and(|(_, value)| best.is_none_or(|(_, current)| value > current));
        if improves {
            best = candidate;
        }

        if share == 0 {
            break;
        }

        share = (share - 1) & mask;
    }

    best
}

/// Walks the splits back from the agent to the first, picking the best covered task set
/// overall, and collects each agent's bundle.
fn reconstruct(
    best_bundles: &[Vec<Option<Bundle<TaskId>>>],
    splits: &[Vec<Option<(usize, u64)>>],
    tasks: usize,
) -> Assignment {
    let Some(last) = splits.last() else {
        return Assignment::empty(0);
    };

    let mut mask = (0..1_usize << tasks)
        .filter(|&mask| last[mask].is_some())
        .max_by_key(|&mask| (last[mask].map(|(_, value)| value), core::cmp::Reverse(mask)))
        .unwrap_or(0);

    let mut bundles: Vec<Bundle<TaskId>> = Vec::with_capacity(splits.len());
    for (agent, layer) in splits.iter().enumerate().rev() {
        let (share, _) = layer[mask].expect("every reachable set has a split");
        let bundle = best_bundles[agent][share]
            .clone()
            .expect("splits only use buildable sets");
        bundles.push(bundle);
        mask &= !share;
    }

    bundles.reverse();
    Assignment::from_bundles(bundles)
}
