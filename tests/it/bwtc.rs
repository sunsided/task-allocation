//! BW-TC over simulated networks that deliver every message: runs settle, and do so on
//! conflict-free assignments that every agent agrees on and nobody can outbid.

use proptest::prelude::*;
use task_allocation::bwtc::{Agent, Bid};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, TaskId, Valuation};

use crate::acbba::lossless;
use crate::bundle::gains_replay;
use crate::cbba::{MAX_TEAM, SETTLE_TICKS};
use crate::support::{self, Instance, instance_with};

/// A BW-TC team for `instance`.
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

/// Runs BW-TC for `instance` on `network` until it settles; `None` when it does not within
/// [`SETTLE_TICKS`].
fn run(instance: &Instance, network: Network) -> Option<Simulation<Agent<support::Agent>>> {
    let mut simulation = Simulation::new(team(instance), network);
    simulation.run_until_settled(SETTLE_TICKS)?;

    Some(simulation)
}

/// Checks a settled team: no conflicts, every agent knows each task's real holder and
/// bid, honest gains, and no agent with room can outbid a holder or take a free task.
pub fn stable(
    simulation: &Simulation<Agent<support::Agent>>,
    instance: &Instance,
) -> Result<(), TestCaseError> {
    let assignment = simulation.assignment();
    prop_assert!(assignment.is_conflict_free());

    for agent in simulation.agents() {
        for task in (0..instance.tasks).map(TaskId) {
            let held = assignment.holder(task).map(|holder| {
                let gain = assignment
                    .bundle(holder)
                    .entries()
                    .find(|(held, _)| **held == task)
                    .map_or(0, |(_, gain)| gain);
                Bid {
                    bidder: holder,
                    gain,
                }
            });
            prop_assert_eq!(agent.best(task), held);
        }

        let bundle = agent.bundle();
        prop_assert!(bundle.len() <= instance.capacity);
        gains_replay(agent.valuation(), bundle)?;
        if bundle.len() < instance.capacity {
            for task in (0..instance.tasks)
                .map(TaskId)
                .filter(|task| !bundle.contains(task))
            {
                let Some(insertion) = agent.valuation().best_insertion(bundle.path(), &task) else {
                    continue;
                };
                let mine = Bid {
                    bidder: agent.id(),
                    gain: insertion.gain,
                };
                let beats =
                    insertion.gain > 0 && agent.best(task).is_none_or(|best| mine.outbids(best));
                prop_assert!(!beats, "{:?} could outbid on {:?}", agent.id(), task);
            }
        }
    }

    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Without losses, every run settles on a stable assignment, on complete and sparse
    /// topologies with delays that reorder messages.
    #[test]
    fn lossless_runs_settle(
        (instance, network) in instance_with(MAX_TEAM).prop_flat_map(|instance| {
            let agents = instance.agents.len();
            (Just(instance), lossless(agents))
        }),
    ) {
        let Some(simulation) = run(&instance, network) else {
            return Err(TestCaseError::fail("did not settle"));
        };

        stable(&simulation, &instance)?;
    }
}

/// A coverage agent over `covers`, with `weights` and the tasks `allowed` lets it take.
fn coverage(covers: Vec<u16>, weights: Vec<u64>, allowed: u32) -> support::Agent {
    support::Agent::Coverage(support::Coverage {
        covers,
        weights,
        allowed,
    })
}

/// A courier starting at `start`, visiting `sites` for `rewards`.
fn courier(start: u64, sites: &[(u64, u64, u64)], rewards: Vec<u64>) -> support::Agent {
    let sites = sites
        .iter()
        .map(|&(position, duration, deadline)| support::Site {
            position,
            duration,
            deadline,
        })
        .collect();

    support::Agent::Courier(task_allocation::BestInsertion(support::Courier {
        start,
        sites,
        rewards,
    }))
}

/// Six agents on a complete network. With one bid kept per task, as published, agent 4
/// discards agent 3's bid on task 4 while a better one stands, later bids on the freed task
/// itself, and ignores the others' answers, whose time it has already seen: the team falls
/// quiet with agent 4 believing it wins a task agent 3 holds.
#[test]
fn discarded_bids_come_back_when_the_best_is_dropped() {
    let instance = Instance {
        agents: vec![
            coverage(
                vec![0, 0, 0, 155, 26],
                vec![19, 19, 16, 1, 19, 6, 16, 2],
                555_907_097,
            ),
            coverage(
                vec![198, 204, 72, 25, 197],
                vec![4, 13, 9, 7, 18, 17, 17, 19],
                1_761_490_529,
            ),
            coverage(
                vec![46, 169, 90, 175, 149],
                vec![12, 10, 17, 16, 4, 9, 5, 15],
                4_143_711_347,
            ),
            coverage(
                vec![72, 52, 118, 186, 102],
                vec![3, 1, 5, 6, 19, 12, 11, 4],
                4_041_653_145,
            ),
            courier(
                16,
                &[
                    (39, 14, 187),
                    (50, 14, 178),
                    (32, 8, 231),
                    (37, 14, 113),
                    (5, 1, 243),
                ],
                vec![13, 10, 29, 23, 3],
            ),
            coverage(
                vec![102, 60, 73, 70, 12],
                vec![7, 7, 16, 15, 8, 5, 18, 12],
                1_198_303_813,
            ),
        ],
        tasks: 5,
        capacity: 5,
    };
    let network = Network {
        seed: 1_074_557_508_531_120_924,
        max_delay: 2,
        ..Network::default()
    };

    let simulation = run(&instance, network).expect("settles");
    stable(&simulation, &instance).expect("stable");
}

/// Three agents on a line. When agent 0's bid on task 0 is dropped, agent 2's runner-up
/// bid takes over at agent 1, but agent 0 never heard of it, since agent 1 answered agent
/// 2's bid with the better one at the time; agent 1 now announces it along with the drop.
#[test]
fn runner_up_bids_are_announced_with_the_drop() {
    let instance = Instance {
        agents: vec![
            coverage(
                vec![65, 225, 134, 168, 88, 47],
                vec![3, 5, 9, 3, 2, 8, 15, 19],
                4_055_615_673,
            ),
            coverage(
                vec![54, 61, 181, 186, 12, 114],
                vec![3, 10, 6, 18, 5, 19, 11, 16],
                2_491_619_579,
            ),
            courier(
                63,
                &[
                    (38, 1, 58),
                    (56, 4, 30),
                    (79, 18, 285),
                    (13, 2, 233),
                    (27, 17, 277),
                    (33, 4, 155),
                ],
                vec![11, 5, 27, 17, 14, 8],
            ),
        ],
        tasks: 6,
        capacity: 5,
    };
    let network = Network {
        seed: 7_548_525_644_882_666_869,
        max_delay: 2,
        topology: task_allocation::sim::Topology::Links(vec![
            (AgentId(0), AgentId(1)),
            (AgentId(1), AgentId(2)),
        ]),
        ..Network::default()
    };

    let simulation = run(&instance, network).expect("settles");
    stable(&simulation, &instance).expect("stable");
}

/// The comparison matrix's lossless cases, ACBBA's failures among them, all settle on
/// stable assignments under BW-TC. Run with `ACBBA_CASES=20000`; the default is 500.
///
/// The cases are the matrix's own: `table_matrix` draws them from the same strategy with
/// the same deterministic runner, one tree per case, so both see the same sequence.
#[test]
#[ignore = "an experiment; run on demand"]
fn matrix_cases_settle() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let cases: usize = std::env::var("ACBBA_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(500);
    let strategy = instance_with(MAX_TEAM).prop_flat_map(|instance| {
        let agents = instance.agents.len();
        (Just(instance), lossless(agents))
    });
    let mut runner = TestRunner::deterministic();

    for case in 0..cases {
        let (instance, network) = strategy.new_tree(&mut runner).expect("generates").current();
        let simulation =
            run(&instance, network).unwrap_or_else(|| panic!("case {case} did not settle"));
        stable(&simulation, &instance).unwrap_or_else(|error| panic!("case {case}: {error}"));
    }
}
