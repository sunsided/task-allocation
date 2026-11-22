//! CBBA over unreliable simulated networks: runs settle on conflict-free assignments that
//! agree everywhere, with honest bids that nobody can beat.

use proptest::collection::vec;
use proptest::prelude::*;
use task_allocation::cbba::Agent;
use task_allocation::centralized::optimal;
use task_allocation::sim::{Network, Partition, Simulation};
use task_allocation::{AgentId, Assignment, TaskId, Valuation};

use crate::bundle::gains_replay;
use crate::support::{self, Instance, instance_with};

/// The most agents in a generated team; enough for messages relayed through third parties.
const MAX_TEAM: usize = 6;

/// Ticks after the last partition heals within which every run must settle.
const SETTLE_TICKS: u64 = 2_000;

/// A partition of `agents` agents into up to three groups, active for ticks
/// `from..from + length`.
fn partition(agents: usize) -> impl Strategy<Value = Partition> {
    (0_u64..40, 1_u64..60, vec(0_usize..3, agents)).prop_map(|(from, length, groups)| Partition {
        from,
        until: from + length,
        groups,
    })
}

/// A network with up to 5 ticks of extra delay, up to 50% loss, and up to three partitions.
fn network(agents: usize) -> impl Strategy<Value = Network> {
    (
        any::<u64>(),
        0_u64..=5,
        0_u16..=500,
        vec(partition(agents), 0..=3),
    )
        .prop_map(|(seed, max_delay, loss_per_mille, partitions)| Network {
            seed,
            max_delay,
            loss_per_mille,
            partitions,
        })
}

/// An instance of up to [`MAX_TEAM`] agents together with a network for them.
fn scenario() -> impl Strategy<Value = (Instance, Network)> {
    instance_with(MAX_TEAM).prop_flat_map(|instance| {
        let agents = instance.agents.len();

        (Just(instance), network(agents))
    })
}

/// A CBBA team for `instance`.
fn team(instance: &Instance) -> Vec<Agent<support::Agent>> {
    let size = instance.agents.len();

    instance
        .agents
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, valuation)| {
            Agent::new(
                AgentId(id),
                size,
                instance.tasks,
                instance.capacity,
                valuation,
            )
        })
        .collect()
}

/// Runs CBBA for `instance` on `network` until it settles; `None` when it does not settle
/// within [`SETTLE_TICKS`] of the last partition healing.
fn run(instance: &Instance, network: Network) -> Option<Simulation<support::Agent>> {
    let healed = network
        .partitions
        .iter()
        .map(|partition| partition.until)
        .max()
        .unwrap_or(0);
    let mut simulation = Simulation::new(team(instance), network);
    simulation.run_until_settled(healed + SETTLE_TICKS)?;

    Some(simulation)
}

/// Checks that every agent believes the holder of each task wins it, with the bid the
/// holder recorded.
fn agrees_with(
    simulation: &Simulation<support::Agent>,
    assignment: &Assignment,
) -> Result<(), TestCaseError> {
    for agent in simulation.agents() {
        for (task, winner) in agent.winners().enumerate() {
            let task = TaskId(task);
            prop_assert_eq!(winner, assignment.holder(task));

            let recorded = winner.and_then(|holder| {
                assignment
                    .bundle(holder)
                    .entries()
                    .find(|(held, _)| **held == task)
                    .map(|(_, gain)| gain)
            });
            prop_assert_eq!(agent.winning_bid(task), recorded.unwrap_or(0));
        }
    }

    Ok(())
}

/// Checks that no agent with room left could outbid the winner of a task it does not hold.
fn nobody_can_outbid(
    simulation: &Simulation<support::Agent>,
    instance: &Instance,
) -> Result<(), TestCaseError> {
    for agent in simulation.agents() {
        let bundle = agent.bundle();
        if bundle.len() >= instance.capacity {
            continue;
        }

        for task in (0..instance.tasks)
            .map(TaskId)
            .filter(|task| !bundle.contains(task))
        {
            let Some(insertion) = agent.valuation().best_insertion(bundle.path(), &task) else {
                continue;
            };

            let winning = agent.winning_bid(task);
            let beats = match agent.winner(task) {
                None => insertion.gain > 0,
                Some(winner) => {
                    insertion.gain > winning || (insertion.gain == winning && agent.id() < winner)
                }
            };
            prop_assert!(
                !beats,
                "{:?} could outbid {:?} on {:?}",
                agent.id(),
                agent.winner(task),
                task
            );
        }
    }

    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Every run settles on a conflict-free assignment that all agents agree on, whose bids
    /// are the holders' real marginal gains, that no agent can improve on by bidding, and
    /// that never beats the optimum.
    #[test]
    fn settles_on_a_stable_assignment((instance, network) in scenario()) {
        let Some(simulation) = run(&instance, network) else {
            return Err(TestCaseError::fail("did not settle"));
        };
        let assignment = simulation.assignment();

        prop_assert!(assignment.is_conflict_free());
        agrees_with(&simulation, &assignment)?;
        nobody_can_outbid(&simulation, &instance)?;
        for (agent, valuation) in instance.agents.iter().enumerate() {
            prop_assert!(assignment.bundle(AgentId(agent)).len() <= instance.capacity);
            gains_replay(valuation, assignment.bundle(AgentId(agent)))?;
        }

        let best = optimal(&instance.agents, instance.tasks, instance.capacity);
        prop_assert!(assignment.value() <= best.value());
    }

    /// Once settled, nothing changes, however long the team keeps talking.
    #[test]
    fn settled_teams_stay_settled((instance, network) in scenario()) {
        let Some(mut simulation) = run(&instance, network) else {
            return Err(TestCaseError::fail("did not settle"));
        };
        let assignment = simulation.assignment();

        for _ in 0..20 {
            simulation.step();
            prop_assert!(simulation.is_settled());
        }

        prop_assert_eq!(simulation.assignment(), assignment);
    }
}

/// The holder of every task, in task order.
fn holders(assignment: &Assignment, tasks: usize) -> Vec<Option<AgentId>> {
    (0..tasks)
        .map(|task| assignment.holder(TaskId(task)))
        .collect()
}

/// CBBA can settle away from the sequential greedy assignment even on a perfect network.
///
/// Agent 0 first bids 33 on task 1, a bid that assumes it also wins task 0. Agent 1 is
/// outbid on task 1 and fills the slot with task 2 instead. Agent 0 then loses task 0 and
/// withdraws to a bid of 10 on task 1, but nobody outbids agent 1 on task 2, so the slot
/// stays taken, and behind task 2, task 1 is worth less than 10 to agent 1.
#[test]
fn blocked_slot_differs_from_greedy() {
    let instance = Instance {
        agents: vec![
            support::Agent::Coverage(support::Coverage {
                covers: vec![81, 172, 156, 182, 141],
                weights: vec![15, 14, 3, 8, 11, 10, 14, 12],
                allowed: 2_422_693_623,
            }),
            support::Agent::Coverage(support::Coverage {
                covers: vec![107, 198, 184, 161, 28],
                weights: vec![18, 19, 10, 1, 9, 8, 15, 11],
                allowed: 2_274_905_959,
            }),
        ],
        tasks: 5,
        capacity: 4,
    };

    let simulation = run(&instance, Network::default()).expect("settles");
    let greedy = task_allocation::centralized::sequential_greedy(&instance.agents, 5, 4);
    let cbba = simulation.assignment();

    let (a, b) = (Some(AgentId(0)), Some(AgentId(1)));
    assert_eq!(holders(&greedy, 5), [b, b, a, None, a]);
    assert_eq!(holders(&cbba, 5), [b, a, b, None, a]);
    assert_eq!((greedy.value(), cbba.value()), (131, 129));
}

/// Equal gains can be resolved differently than greedy resolves them: greedy takes the
/// smaller of two tasks worth 25 to agent 0, while CBBA, after a lost message, takes the
/// other one first, which makes the first one infeasible.
#[test]
fn tied_gains_can_differ_from_greedy() {
    let site = |position, duration, deadline| support::Site {
        position,
        duration,
        deadline,
    };
    let instance = Instance {
        agents: vec![
            support::Agent::Courier(task_allocation::BestInsertion(support::Courier {
                start: 27,
                sites: vec![
                    site(36, 12, 108),
                    site(83, 11, 66),
                    site(28, 19, 20),
                    site(1, 15, 263),
                    site(34, 13, 108),
                    site(74, 7, 93),
                ],
                rewards: vec![25, 29, 27, 25, 26, 3],
            })),
            support::Agent::Coverage(support::Coverage {
                covers: vec![76, 46, 70, 124, 12, 79],
                weights: vec![16, 19, 13, 14, 15, 11, 7, 5],
                allowed: 2_458_217_047,
            }),
            support::Agent::Coverage(support::Coverage {
                covers: vec![198, 43, 46, 145, 40, 155],
                weights: vec![8, 9, 3, 10, 1, 18, 13, 17],
                allowed: 3_524_159_223,
            }),
        ],
        tasks: 6,
        capacity: 6,
    };
    let network = Network {
        seed: 1_086_238_513_920_705_783,
        loss_per_mille: 129,
        ..Network::default()
    };

    let simulation = run(&instance, network).expect("settles");
    let greedy = task_allocation::centralized::sequential_greedy(&instance.agents, 6, 6);
    let cbba = simulation.assignment();

    assert_eq!(greedy.holder(TaskId(0)), Some(AgentId(0)));
    assert_eq!(cbba.holder(TaskId(0)), Some(AgentId(2)));
    assert_eq!(holders(&greedy, 6)[1..], holders(&cbba, 6)[1..]);
}
