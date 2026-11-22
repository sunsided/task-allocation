//! The auction baselines against the exhaustive optimum and brute-force winner
//! determination.

use proptest::collection::vec;
use proptest::prelude::*;
use task_allocation::auction::{
    BundleBid, bundle_bids, combinatorial, sequential, winner_determination,
};
use task_allocation::centralized::optimal;
use task_allocation::{AgentId, Assignment, Bundle, Insertion, TaskId, Valuation};

use crate::support::{Instance, instance};

/// A bid of `agent` on the tasks in `mask` (bits over tasks), worth `value`.
fn bid(agent: usize, mask: u8, value: u64) -> BundleBid {
    let mut bundle = Bundle::new();
    for (position, task) in (0..8).filter(|task| mask & (1 << task) != 0).enumerate() {
        let gain = if position == 0 { value } else { 0 };
        bundle.insert(TaskId(task), Insertion { position, gain });
    }

    BundleBid {
        agent: AgentId(agent),
        bundle,
    }
}

/// The best value of any selection of at most one bid per agent with disjoint tasks, by
/// trying every subset of bids.
fn brute_force_value(bids: &[BundleBid]) -> u64 {
    let mut best = 0;
    for subset in 0_u32..(1 << bids.len()) {
        let chosen: Vec<&BundleBid> = (0..bids.len())
            .filter(|bid| subset & (1 << bid) != 0)
            .map(|bid| &bids[bid])
            .collect();
        let mut agents: Vec<AgentId> = chosen.iter().map(|bid| bid.agent).collect();
        let mut tasks: Vec<TaskId> = chosen
            .iter()
            .flat_map(|bid| bid.bundle.tasks().copied())
            .collect();
        let (agent_count, task_count) = (agents.len(), tasks.len());
        agents.sort_unstable();
        agents.dedup();
        tasks.sort_unstable();
        tasks.dedup();

        if agents.len() == agent_count && tasks.len() == task_count {
            best = best.max(chosen.iter().map(|bid| bid.value()).sum());
        }
    }

    best
}

/// Checks that `assignment` has one bundle per agent, within capacity, and no conflicts.
fn well_formed(instance: &Instance, assignment: &Assignment) -> Result<(), TestCaseError> {
    prop_assert_eq!(assignment.agents(), instance.agents.len());
    prop_assert!(assignment.is_conflict_free());
    for bundle in assignment.bundles() {
        prop_assert!(bundle.len() <= instance.capacity);
    }

    Ok(())
}

proptest! {
    /// A sequential auction in any order is well formed and never beats the optimum.
    #[test]
    fn sequential_never_beats_the_optimum(instance in instance(), order in vec(0_usize..8, 0..12)) {
        let order = order.into_iter().map(TaskId);
        let assignment = sequential(&instance.agents, instance.tasks, instance.capacity, order);
        let best = optimal(&instance.agents, instance.tasks, instance.capacity);

        well_formed(&instance, &assignment)?;
        prop_assert!(assignment.value() <= best.value());
    }

    /// With every bundle bid, the combinatorial auction finds the optimum; with fewer bids,
    /// it never beats it.
    #[test]
    fn combinatorial_reaches_the_optimum(instance in instance(), few in 1_usize..4) {
        let best = optimal(&instance.agents, instance.tasks, instance.capacity);
        let full = combinatorial(&instance.agents, instance.tasks, instance.capacity, usize::MAX);
        let limited = combinatorial(&instance.agents, instance.tasks, instance.capacity, few);

        well_formed(&instance, &full)?;
        well_formed(&instance, &limited)?;
        prop_assert_eq!(full.value(), best.value());
        prop_assert!(limited.value() <= best.value());
    }

    /// Winner determination matches brute force over every subset of bids.
    #[test]
    fn winner_determination_matches_brute_force(
        raw in vec((0_usize..3, 1_u8..=255, 1_u64..50), 0..10),
    ) {
        let bids: Vec<BundleBid> = raw.iter().map(|&(agent, mask, value)| bid(agent, mask, value)).collect();
        let assignment = winner_determination(&bids, 3);

        prop_assert!(assignment.is_conflict_free());
        prop_assert_eq!(assignment.value(), brute_force_value(&bids));
    }

    /// Bundle bids come most valuable first, within the size limit, each bundle once, and
    /// each with a positive value.
    #[test]
    fn bundle_bids_are_ranked(instance in instance(), max_size in 1_usize..4, limit in 1_usize..20) {
        for (index, valuation) in instance.agents.iter().enumerate() {
            let bids = bundle_bids(AgentId(index), valuation, instance.tasks, max_size, limit);

            prop_assert!(bids.len() <= limit);
            prop_assert!(bids.windows(2).all(|pair| pair[0].value() >= pair[1].value()));
            prop_assert!(bids.iter().all(|bid| bid.value() > 0 && bid.bundle.len() <= max_size));
            prop_assert!(bids.iter().all(|bid| bid.agent == AgentId(index)));

            let mut sets: Vec<Vec<TaskId>> = bids.iter().map(|bid| {
                let mut tasks = bid.bundle.path().to_vec();
                tasks.sort_unstable();
                tasks
            }).collect();
            let count = sets.len();
            sets.sort();
            sets.dedup();
            prop_assert_eq!(sets.len(), count);
        }
    }
}

/// Bids from agents outside the team are ignored, and so are sparse task identifiers.
#[test]
fn out_of_range_bids_are_ignored() {
    let assignment = winner_determination(&[bid(5, 1, 9), bid(0, 2, 3)], 2);
    assert_eq!(assignment.value(), 3);

    let mut far = Bundle::new();
    far.insert(
        TaskId(usize::MAX),
        Insertion {
            position: 0,
            gain: 4,
        },
    );
    let sparse = BundleBid {
        agent: AgentId(1),
        bundle: far,
    };
    assert_eq!(winner_determination(&[sparse], 2).value(), 4);
}

/// Wants task 0 only after task 1, and takes any other task, even unknown ones.
struct AfterOne;

impl Valuation<TaskId> for AfterOne {
    fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
        (task.index() != 0 || path.contains(&TaskId(1))).then_some(1)
    }
}

/// Unknown tasks are skipped, and a task listed twice is auctioned only once, even when
/// nobody bid on it the first time.
#[test]
fn each_known_task_is_auctioned_once() {
    let auctioned = sequential(
        &[AfterOne],
        2,
        3,
        [TaskId(7), TaskId(0), TaskId(1), TaskId(0)],
    );

    assert_eq!(auctioned.holder(TaskId(0)), None);
    assert_eq!(auctioned.holder(TaskId(1)), Some(AgentId(0)));
    assert_eq!(auctioned.bundle(AgentId(0)).len(), 1);
}
