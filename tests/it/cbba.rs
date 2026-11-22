//! CBBA over unreliable simulated networks: runs settle on conflict-free assignments that
//! agree everywhere, with honest bids that nobody can beat.

use proptest::collection::vec;
use proptest::prelude::*;
use task_allocation::cbba::{Agent, Replan};
use task_allocation::centralized::optimal;
use task_allocation::sim::{Network, Partition, Simulation, Topology};
use task_allocation::{AgentId, Assignment, TaskId, Valuation};

use crate::bundle::gains_replay;
use crate::support::{self, Instance, instance_sized, instance_with, is_greedy_outcome};

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

/// A connected topology over `agents` agents: complete, or a random spanning tree with a
/// few extra links, so that some agents hear of each other only through relays.
fn topology(agents: usize) -> impl Strategy<Value = Topology> {
    let parents = (1..agents.max(2)).map(|agent| 0..agent).collect::<Vec<_>>();
    let tree = parents.prop_map(|parents| {
        parents
            .into_iter()
            .enumerate()
            .map(|(index, parent)| (AgentId(parent), AgentId(index + 1)))
            .collect::<Vec<_>>()
    });
    let extra = vec((0..agents, 0..agents), 0..agents);
    let sparse = (tree, extra).prop_map(|(mut links, extra)| {
        links.extend(extra.into_iter().map(|(a, b)| (AgentId(a), AgentId(b))));
        Topology::Links(links)
    });

    prop_oneof![Just(Topology::Complete), sparse]
}

/// A network with up to 5 ticks of extra delay, up to 50% loss, a connected topology, and
/// up to three partitions.
fn network(agents: usize) -> impl Strategy<Value = Network> {
    (
        any::<u64>(),
        0_u64..=5,
        0_u16..=500,
        topology(agents),
        vec(partition(agents), 0..=3),
    )
        .prop_map(
            |(seed, max_delay, loss_per_mille, topology, partitions)| Network {
                seed,
                max_delay,
                loss_per_mille,
                topology,
                partitions,
            },
        )
}

/// An instance of up to [`MAX_TEAM`] agents together with a network for them.
fn scenario() -> impl Strategy<Value = (Instance, Network)> {
    with_network(instance_with(MAX_TEAM))
}

/// `instances`, each together with a network for its agents.
fn with_network(
    instances: impl Strategy<Value = Instance>,
) -> impl Strategy<Value = (Instance, Network)> {
    instances.prop_flat_map(|instance| {
        let agents = instance.agents.len();

        (Just(instance), network(agents))
    })
}

/// A CBBA team for `instance`.
fn team(instance: &Instance, replan: Replan) -> Vec<Agent<support::Agent>> {
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
            .with_replan(replan)
        })
        .collect()
}

/// Runs CBBA for `instance` on `network` until it settles; `None` when it does not settle
/// within [`SETTLE_TICKS`] of the last partition healing.
fn run(instance: &Instance, network: Network) -> Option<Simulation<Agent<support::Agent>>> {
    run_with(instance, network, Replan::Keep)
}

/// Runs CBBA for `instance` on `network`, with every agent replanning as `replan` says,
/// until it settles; `None` when it does not settle within [`SETTLE_TICKS`] of the last
/// partition healing.
fn run_with(
    instance: &Instance,
    network: Network,
    replan: Replan,
) -> Option<Simulation<Agent<support::Agent>>> {
    let healed = network
        .partitions
        .iter()
        .map(|partition| partition.until)
        .max()
        .unwrap_or(0);
    let mut simulation = Simulation::new(team(instance, replan), network);
    simulation.run_until_settled(healed + SETTLE_TICKS)?;

    Some(simulation)
}

/// Checks that every agent believes the holder of each task wins it, with the bid the
/// holder recorded.
fn agrees_with(
    simulation: &Simulation<Agent<support::Agent>>,
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
    simulation: &Simulation<Agent<support::Agent>>,
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

/// Checks everything a settled team promises: no conflicts, agreement on winners and bids,
/// honest bids, and no agent able to outbid a winner.
fn stable(
    simulation: &Simulation<Agent<support::Agent>>,
    instance: &Instance,
) -> Result<(), TestCaseError> {
    let assignment = simulation.assignment();

    prop_assert!(assignment.is_conflict_free());
    agrees_with(simulation, &assignment)?;
    nobody_can_outbid(simulation, instance)?;
    for (agent, valuation) in instance.agents.iter().enumerate() {
        prop_assert!(assignment.bundle(AgentId(agent)).len() <= instance.capacity);
        gains_replay(valuation, assignment.bundle(AgentId(agent)))?;
    }

    Ok(())
}

/// Checks that `simulation` stays settled, with the same assignment, for 20 more ticks.
fn stays_settled(simulation: &mut Simulation<Agent<support::Agent>>) -> Result<(), TestCaseError> {
    let assignment = simulation.assignment();
    for _ in 0..20 {
        simulation.step();
        prop_assert!(simulation.is_settled());
    }

    prop_assert_eq!(simulation.assignment(), assignment);

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

    /// Larger instances, too big for the exhaustive optimum, settle on stable assignments.
    #[test]
    fn larger_instances_settle((instance, network) in with_network(instance_sized(MAX_TEAM, 14))) {
        let Some(simulation) = run(&instance, network) else {
            return Err(TestCaseError::fail("did not settle"));
        };
        let assignment = simulation.assignment();

        prop_assert!(assignment.is_conflict_free());
        agrees_with(&simulation, &assignment)?;
        nobody_can_outbid(&simulation, &instance)?;
        for (agent, valuation) in instance.agents.iter().enumerate() {
            gains_replay(valuation, assignment.bundle(AgentId(agent)))?;
        }
    }

    /// With a full replan before every bundle phase, runs settle on a stable assignment
    /// that some run of the sequential greedy assignment reaches, breaking ties in some
    /// order.
    #[test]
    fn full_replan_reaches_a_greedy_outcome((instance, network) in scenario()) {
        let Some(mut simulation) = run_with(&instance, network, Replan::Full) else {
            return Err(TestCaseError::fail("did not settle"));
        };

        stable(&simulation, &instance)?;
        stays_settled(&mut simulation)?;
        let holders = holders(&simulation.assignment(), instance.tasks);
        prop_assert!(is_greedy_outcome(&instance, &holders), "{:?}", holders);
    }

    /// Releasing only the last few tasks before every bundle phase still settles on a
    /// stable assignment.
    #[test]
    fn partial_replan_settles((instance, network) in scenario(), last in 1_usize..4) {
        let Some(mut simulation) = run_with(&instance, network, Replan::Last(last)) else {
            return Err(TestCaseError::fail("did not settle"));
        };

        stable(&simulation, &instance)?;
        stays_settled(&mut simulation)?;
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

/// This implementation can settle away from the sequential greedy assignment even on a
/// perfect network.
///
/// The gains come from the coverage valuations below, as `task (gain)` in bundle order:
///
/// 1. Round 1: agent 0 builds 0 (40), 1 (33); agent 1 builds 0 (61), 1 (21), 2 (9).
/// 2. Consensus: agent 0 loses task 0 to 61 and releases 0 and 1. Agent 1 sees agent 0's
///    33 on task 1, which beats its 21, and releases 1 and 2.
/// 3. Round 2: agent 1 cannot win task 1 (33 > 21) and adds 2 (20), its gain on task 2
///    behind task 0 alone. Agent 0 builds 4 (38), 2 (11), 1 (10).
/// 4. Consensus: agent 0 loses task 2 to 20 and releases 2 and 1. Round 3: agent 0 builds
///    4 (38), 1 (10).
/// 5. Agent 1 would now bid on task 1, but behind tasks 0 and 2 only item 2 (weight 10) is
///    new to it: a gain of 10, which ties agent 0's 10 and loses to the smaller agent. Its
///    slot stays filled with task 2.
///
/// Greedy instead gives agent 1 task 1 at 21 right after task 0 (items 2 and 7, weights 10
/// and 11), and agent 0 task 2: a total of 131 rather than 129.
#[test]
fn blocked_slot_differs_from_greedy() {
    let instance = blocked_slot_instance();

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

/// A full replan frees the blocked slot: the same instance reaches the greedy assignment.
#[test]
fn full_replan_frees_the_blocked_slot() {
    let instance = blocked_slot_instance();
    let simulation = run_with(&instance, Network::default(), Replan::Full).expect("settles");
    let greedy = task_allocation::centralized::sequential_greedy(&instance.agents, 5, 4);

    assert_eq!(holders(&simulation.assignment(), 5), holders(&greedy, 5));
    assert_eq!(simulation.assignment().value(), 131);
}

/// Two coverage agents where agent 1 fills a slot while a bid of agent 0 that is later
/// withdrawn blocks a better task.
fn blocked_slot_instance() -> Instance {
    Instance {
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
    }
}
