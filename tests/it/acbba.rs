//! ACBBA over simulated networks. Runs that settle do so on stable assignments, with or
//! without losses and partitions. Not every run settles, even with re-announcing and
//! without losses (about one in ten thousand; see the comparison matrix and issue #33), and
//! the published rules alone can go quiet without agreeing.

use proptest::prelude::*;
use task_allocation::acbba::{Agent, Rules};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, TaskId, Valuation};

use crate::bundle::gains_replay;
use crate::cbba::{MAX_TEAM, SETTLE_TICKS, network, topology};
use crate::support::{self, Instance, instance_with};

mod matrix;

/// An ACBBA team for `instance` under the 2011 rules, re-announcing every `period` ticks
/// when given one.
fn team(instance: &Instance, period: Option<u64>) -> Vec<Agent<support::Agent>> {
    instance
        .agents
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, valuation)| {
            let agent = Agent::new(AgentId(id), instance.tasks, instance.capacity, valuation)
                .with_rules(Rules::Infotech2011);
            match period {
                Some(period) => agent.announce_every(period),
                None => agent,
            }
        })
        .collect()
}

/// Runs ACBBA until the team settles; `None` when it does not within [`SETTLE_TICKS`] of
/// the last partition healing.
fn run(
    instance: &Instance,
    network: Network,
    period: Option<u64>,
) -> Option<Simulation<Agent<support::Agent>>> {
    let healed = network
        .partitions
        .iter()
        .map(|partition| partition.until)
        .max()
        .unwrap_or(0);
    let mut simulation = Simulation::new(team(instance, period), network);
    simulation.run_until_settled(healed + SETTLE_TICKS)?;

    Some(simulation)
}

/// Checks what a settled team promises: no conflicts, every agent agreeing with the holder
/// and its recorded bid, honest bids, and no agent able to outbid a winner.
fn stable(
    simulation: &Simulation<Agent<support::Agent>>,
    instance: &Instance,
) -> Result<(), TestCaseError> {
    let assignment = simulation.assignment();
    prop_assert!(assignment.is_conflict_free());

    for agent in simulation.agents() {
        for task in (0..instance.tasks).map(TaskId) {
            let claim = agent.claim(task);
            let holder = assignment.holder(task);
            prop_assert_eq!(claim.winner, holder);

            let recorded = holder.and_then(|holder| {
                assignment
                    .bundle(holder)
                    .entries()
                    .find(|(held, _)| **held == task)
                    .map(|(_, gain)| gain)
            });
            prop_assert_eq!(claim.bid, recorded.unwrap_or(0));
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

                let claim = agent.claim(task);
                let beats = insertion.gain > 0
                    && claim.winner.is_none_or(|winner| {
                        insertion.gain > claim.bid
                            || (insertion.gain == claim.bid && agent.id() < winner)
                    });
                prop_assert!(!beats, "{:?} could outbid on {:?}", agent.id(), task);
            }
        }
    }

    Ok(())
}

/// A network without losses or partitions: delays of up to five ticks, which reorder
/// messages, on a complete or sparse topology.
pub fn lossless(agents: usize) -> impl Strategy<Value = Network> {
    (any::<u64>(), 0_u64..=5, topology(agents)).prop_map(|(seed, max_delay, topology)| Network {
        seed,
        max_delay,
        loss_per_mille: 0,
        topology,
        partitions: Vec::new(),
        collisions: false,
        clusters: None,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Without losses, agents that re-announce every ten ticks and settle do so in agreement
    /// on a stable assignment, on complete and sparse topologies. Once settled, the
    /// re-announcements only move bid times, so no copy counts as useful.
    #[test]
    fn settled_lossless_runs_are_stable(
        (instance, network) in instance_with(MAX_TEAM).prop_flat_map(|instance| {
            let agents = instance.agents.len();
            (Just(instance), lossless(agents))
        }),
    ) {
        let Some(mut simulation) = run(&instance, network, Some(10)) else {
            return Ok(());
        };

        stable(&simulation, &instance)?;
        if instance.agents.len() > 1 && simulation.assignment().value() > 0 {
            prop_assert!(simulation.stats().useful > 0);
        }

        let settled = simulation.stats();
        for _ in 0..30 {
            simulation.step();
        }
        prop_assert!(simulation.stats().delivered > settled.delivered || instance.agents.len() < 2);
        prop_assert_eq!(simulation.stats().useful, settled.useful);
    }

    /// With losses and partitions, agents that re-announce their view every ten ticks and
    /// settle do so on a stable assignment.
    #[test]
    fn settled_lossy_runs_are_stable(
        (instance, network) in instance_with(MAX_TEAM).prop_flat_map(|instance| {
            let agents = instance.agents.len();
            (Just(instance), network(agents))
        }),
    ) {
        let Some(simulation) = run(&instance, network, Some(10)) else {
            return Ok(());
        };

        stable(&simulation, &instance)?;
    }
}

/// The instance of [`stale_newer_bid_survives_without_reannouncing`]: six agents, two of
/// them coverage valuations, over six courier sites.
fn stale_bid_instance() -> Instance {
    let site = |position, duration, deadline| support::Site {
        position,
        duration,
        deadline,
    };
    let sites = vec![
        site(27, 15, 93),
        site(3, 19, 110),
        site(44, 18, 254),
        site(51, 9, 254),
        site(67, 8, 77),
        site(29, 6, 67),
    ];
    let courier = |start, rewards: Vec<u64>| {
        support::Agent::Courier(task_allocation::BestInsertion(support::Courier {
            start,
            sites: sites.clone(),
            rewards,
        }))
    };
    Instance {
        agents: vec![
            support::Agent::Coverage(support::Coverage {
                covers: vec![0; 6],
                weights: vec![8, 19, 1, 19, 4, 17, 18, 3],
                allowed: 1_701_689_649,
            }),
            courier(40, vec![10, 16, 3, 27, 20, 9]),
            courier(39, vec![26, 17, 4, 15, 4, 8]),
            courier(74, vec![5, 21, 2, 12, 18, 25]),
            courier(88, vec![20, 1, 13, 5, 24, 27]),
            support::Agent::Coverage(support::Coverage {
                covers: vec![34, 246, 227, 82, 231, 173],
                weights: vec![19, 5, 17, 18, 13, 17, 11, 3],
                allowed: 2_768_660_029,
            }),
        ],
        tasks: 6,
        capacity: 2,
    }
}

/// The published rules alone can fail to agree, even without losses: agent 5
/// keeps agent 1's released bid of 10 on task 0, made at tick 4, because under rows 28
/// and 29 it is newer than agent 2's winning bid of 26 from tick 0. Re-announcing, with
/// winning bids confirmed at the current time, settles the same run.
#[test]
fn stale_newer_bid_survives_without_reannouncing() {
    let instance = stale_bid_instance();
    let link = |a, b| (AgentId(a), AgentId(b));
    let network = Network {
        seed: 14_530_582_918_442_739_863,
        max_delay: 2,
        topology: task_allocation::sim::Topology::Links(vec![
            link(0, 1),
            link(0, 2),
            link(1, 3),
            link(1, 4),
            link(3, 5),
        ]),
        ..Network::default()
    };

    let mut published = Simulation::new(team(&instance, None), network.clone());
    for _ in 0..300 {
        published.step();
    }

    let agents = published.agents();
    assert_eq!(agents[5].claim(TaskId(0)).winner, Some(AgentId(1)));
    assert_eq!(agents[0].claim(TaskId(0)).winner, Some(AgentId(2)));
    assert!(!published.is_settled());

    let reannouncing = run(&instance, network, Some(10)).expect("settles");
    assert_eq!(
        reannouncing.assignment().holder(TaskId(0)),
        Some(AgentId(2))
    );
}
