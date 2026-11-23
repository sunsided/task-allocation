//! CBBA agents that broadcast only after their winner tables change settle on the same
//! kind of stable assignment as agents that broadcast every round, with fewer messages.

use proptest::prelude::*;
use task_allocation::AgentId;
use task_allocation::cbba::{Agent, Cadence};
use task_allocation::sim::{Network, Simulation};

use crate::acbba::lossless;
use crate::cbba::{MAX_TEAM, SETTLE_TICKS, network, stable};
use crate::support::{self, Instance, instance_with};

/// A CBBA team for `instance`, broadcasting as `cadence` says.
fn team(instance: &Instance, cadence: Cadence) -> Vec<Agent<support::Agent>> {
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
            .with_cadence(cadence)
        })
        .collect()
}

/// Runs `instance` on `network` under `cadence` until it settles; `None` when it does not
/// settle within [`SETTLE_TICKS`] of the last partition healing.
fn run(
    instance: &Instance,
    network: Network,
    cadence: Cadence,
) -> Option<Simulation<Agent<support::Agent>>> {
    let healed = network
        .partitions
        .iter()
        .map(|partition| partition.until)
        .max()
        .unwrap_or(0);
    let mut simulation = Simulation::new(team(instance, cadence), network);
    simulation.run_until_settled(healed + SETTLE_TICKS)?;

    Some(simulation)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Without losses, agents that only broadcast after a change settle on a stable
    /// assignment, on complete and sparse topologies, and send no more messages than
    /// agents that broadcast every round.
    #[test]
    fn lossless_runs_settle_with_fewer_messages(
        (instance, network) in instance_with(MAX_TEAM).prop_flat_map(|instance| {
            let agents = instance.agents.len();
            (Just(instance), lossless(agents))
        }),
    ) {
        let on_change = Cadence::OnChange { refresh: None };
        let Some(quiet) = run(&instance, network.clone(), on_change) else {
            return Err(TestCaseError::fail("did not settle"));
        };
        stable(&quiet, &instance)?;

        let every_round = run(&instance, network, Cadence::EveryRound).expect("settles");
        prop_assert!(quiet.stats().sent <= every_round.stats().sent);
    }

    /// With losses and partitions, agents that only broadcast after a change, and refresh
    /// their state after a few silent rounds, still settle on a stable assignment.
    #[test]
    fn refreshing_runs_settle_despite_losses(
        (instance, network) in instance_with(MAX_TEAM).prop_flat_map(|instance| {
            let agents = instance.agents.len();
            (Just(instance), network(agents))
        }),
        refresh in 1_u64..=10,
    ) {
        let Some(simulation) = run(&instance, network, Cadence::OnChange { refresh: Some(refresh) }) else {
            return Err(TestCaseError::fail("did not settle"));
        };

        stable(&simulation, &instance)?;
    }
}
