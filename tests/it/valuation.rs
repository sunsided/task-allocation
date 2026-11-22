//! The generated valuations satisfy what the other properties assume.

use proptest::prelude::*;
use task_allocation::{Valuation, check_diminishing};

use crate::support::instance;

proptest! {
    /// Every generated agent has diminishing marginal gains.
    #[test]
    fn generated_agents_diminish(instance in instance()) {
        let tasks = instance.task_ids();
        for agent in &instance.agents {
            let result = check_diminishing(agent, &tasks, instance.capacity);
            prop_assert!(result.is_ok(), "{}", result.unwrap_err());
        }
    }

    /// The best insertion's gain is the marginal score.
    #[test]
    fn insertion_gain_is_the_marginal_score(instance in instance()) {
        let tasks = instance.task_ids();
        for agent in &instance.agents {
            for task in &tasks {
                let gain = agent.best_insertion(&[], task).map(|insertion| insertion.gain);
                prop_assert_eq!(gain, agent.marginal_score(&[], task));
            }
        }
    }
}
