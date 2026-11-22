//! The sequential greedy assignment against the exhaustive optimum.

use proptest::prelude::*;
use task_allocation::centralized::{optimal, sequential_greedy};
use task_allocation::{AgentId, Assignment, Bundle, TaskId, Valuation};

use crate::support::{Instance, instance};

/// Builds an assignment by giving tasks to agents in the order `picks` lists them, skipping
/// tasks already given, agents at capacity and infeasible insertions.
fn assign_in_order(instance: &Instance, picks: &[(usize, usize)]) -> Assignment {
    let mut bundles: Vec<Bundle<TaskId>> = instance.agents.iter().map(|_| Bundle::new()).collect();
    let mut given = vec![false; instance.tasks];

    for &(agent, task) in picks {
        let agent = agent % instance.agents.len();
        let task = task % instance.tasks;
        let bundle = &mut bundles[agent];
        if given[task] || bundle.len() >= instance.capacity {
            continue;
        }

        if let Some(insertion) = instance.agents[agent].best_insertion(bundle.path(), &TaskId(task))
        {
            bundle.insert(TaskId(task), insertion);
            given[task] = true;
        }
    }

    Assignment::from_bundles(bundles)
}

/// Checks that `assignment` has one bundle per agent, within capacity, and no conflicts.
fn well_formed(instance: &Instance, assignment: &Assignment) -> Result<(), TestCaseError> {
    prop_assert_eq!(assignment.agents(), instance.agents.len());
    prop_assert!(assignment.is_conflict_free());
    for agent in 0..assignment.agents() {
        let bundle = assignment.bundle(AgentId(agent));
        prop_assert!(bundle.len() <= instance.capacity);
        prop_assert!(bundle.tasks().all(|task| task.index() < instance.tasks));
    }

    Ok(())
}

proptest! {
    /// Both references return well-formed assignments, and greedy never beats the optimum.
    #[test]
    fn optimum_bounds_greedy(instance in instance()) {
        let greedy = sequential_greedy(&instance.agents, instance.tasks, instance.capacity);
        let best = optimal(&instance.agents, instance.tasks, instance.capacity);

        well_formed(&instance, &greedy)?;
        well_formed(&instance, &best)?;
        prop_assert!(greedy.value() <= best.value());
    }

    /// With diminishing gains, greedy scores at least half the optimum.
    #[test]
    fn greedy_scores_half_the_optimum(instance in instance()) {
        let greedy = sequential_greedy(&instance.agents, instance.tasks, instance.capacity);
        let best = optimal(&instance.agents, instance.tasks, instance.capacity);

        prop_assert!(2 * greedy.value() >= best.value(), "{} vs {}", greedy.value(), best.value());
    }

    /// No assignment built in any order beats the optimum.
    #[test]
    fn optimum_beats_any_build_order(
        instance in instance(),
        picks in proptest::collection::vec((0_usize..8, 0_usize..8), 0..24),
    ) {
        let other = assign_in_order(&instance, &picks);
        let best = optimal(&instance.agents, instance.tasks, instance.capacity);

        prop_assert!(other.value() <= best.value());
    }
}
