//! Invariants of greedy bundle construction.

use proptest::prelude::*;
use task_allocation::{Bundle, TaskId, Valuation};

use crate::support::{Agent, instance};

/// The path of the first `count` tasks of `bundle`: the path with every later task removed.
pub fn prefix_path(bundle: &Bundle<TaskId>, count: usize) -> Vec<TaskId> {
    let prefix: Vec<TaskId> = bundle.tasks().take(count).copied().collect();

    bundle
        .path()
        .iter()
        .filter(|task| prefix.contains(task))
        .copied()
        .collect()
}

/// Checks that every recorded gain is what the agent reports for that task on the path of
/// the tasks added before it.
pub fn gains_replay(agent: &Agent, bundle: &Bundle<TaskId>) -> Result<(), TestCaseError> {
    for (index, (task, gain)) in bundle.entries().enumerate() {
        let path = prefix_path(bundle, index);
        let insertion = agent.best_insertion(&path, task);
        prop_assert_eq!(insertion.map(|insertion| insertion.gain), Some(gain));
        prop_assert_eq!(prefix_path(bundle, index + 1), {
            let mut expected = path.clone();
            expected.insert(insertion.unwrap().position, *task);
            expected
        });
    }

    Ok(())
}

proptest! {
    /// Greedy bundles hold each task once, respect the capacity, keep the path a permutation
    /// of the bundle, and replay to the recorded gains.
    #[test]
    fn greedy_bundles_are_consistent(instance in instance()) {
        let tasks = instance.task_ids();
        for agent in &instance.agents {
            let mut bundle = Bundle::new();
            bundle.extend_greedily(agent, &tasks, instance.capacity, |_, gain| gain > 0);

            prop_assert!(bundle.len() <= instance.capacity);

            let mut held: Vec<TaskId> = bundle.tasks().copied().collect();
            let mut path = bundle.path().to_vec();
            held.sort_unstable();
            path.sort_unstable();
            held.dedup();
            prop_assert_eq!(held.len(), bundle.len());
            prop_assert_eq!(&held, &path);

            gains_replay(agent, &bundle)?;
        }
    }

    /// With diminishing gains, each task added gains at most as much as the one before.
    #[test]
    fn greedy_gains_never_increase(instance in instance()) {
        let tasks = instance.task_ids();
        for agent in &instance.agents {
            let mut bundle = Bundle::new();
            bundle.extend_greedily(agent, &tasks, instance.capacity, |_, _| true);

            let gains: Vec<u64> = bundle.entries().map(|(_, gain)| gain).collect();
            prop_assert!(gains.windows(2).all(|pair| pair[0] >= pair[1]), "{:?}", gains);
        }
    }

    /// Releasing from an index keeps exactly the tasks added before it, with their path.
    #[test]
    fn release_keeps_the_prefix(instance in instance(), cut in 0_usize..8) {
        let tasks = instance.task_ids();
        for agent in &instance.agents {
            let mut bundle = Bundle::new();
            bundle.extend_greedily(agent, &tasks, instance.capacity, |_, _| true);

            let before: Vec<TaskId> = bundle.tasks().copied().collect();
            let expected_path = prefix_path(&bundle, cut);
            let released = bundle.release_from(cut);

            let kept: Vec<TaskId> = bundle.tasks().copied().collect();
            prop_assert_eq!([kept.as_slice(), released.as_slice()].concat(), before);
            prop_assert_eq!(bundle.path(), expected_path.as_slice());
        }
    }
}
