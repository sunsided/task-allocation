//! BW-TC under every lossless message schedule on tiny teams: from every reachable state,
//! lossless rounds settle the team on a stable assignment.

use task_allocation::AgentId;
use task_allocation::bwtc::Agent;
use task_allocation::sim::{Network, Simulation, Topology};

use super::{Bounds, State, explore};
use crate::bwtc::stable;
use crate::explore::cbba::instances;
use crate::support::{self, Instance};

/// Rounds the closing run may take to settle.
const CLOSE_LIMIT: u32 = 60;

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

/// Explores `instance` on `topology` within `bounds`; every closing run must settle on a
/// stable assignment. Returns the states explored.
fn check(instance: &Instance, topology: &Topology, bounds: Bounds) -> Result<usize, String> {
    let network = Network {
        topology: topology.clone(),
        ..Network::default()
    };
    let report = explore(State::new(team(instance)), topology, bounds, |state| {
        let mut closing = state.clone();
        closing
            .close(topology, CLOSE_LIMIT)
            .ok_or_else(|| format!("did not settle after\n  {}", state.trace.join("\n  ")))?;
        let simulation = Simulation::new(closing.agents, network.clone());
        stable(&simulation, instance).map_err(|error| error.to_string())
    })?;

    Ok(report.states)
}

/// `count` instances of `agents` agents over `tasks` tasks on `topology` settle from every
/// lossless schedule of `acts` rounds per agent.
fn check_all(agents: usize, tasks: usize, topology: &Topology, acts: u32, count: usize) {
    let bounds = Bounds { acts, losses: 0 };
    for instance in instances(agents, tasks, count) {
        let states = check(&instance, topology, bounds)
            .unwrap_or_else(|error| panic!("{instance:?}: {error}"));
        println!("bwtc {agents} agents, {tasks} tasks, {acts} rounds: {states} states");
    }
}

/// Three agents on a line, where the ends hear each other's bids and drops only through
/// the middle.
fn line() -> Topology {
    Topology::Links(vec![(AgentId(0), AgentId(1)), (AgentId(1), AgentId(2))])
}

/// Two agents for two rounds each, and three on a line for one, settle from every
/// lossless schedule.
#[test]
fn small_teams_settle_from_every_schedule() {
    check_all(2, 3, &Topology::Complete, 2, 2);
    check_all(3, 2, &line(), 1, 3);
}

/// More instances, more tasks and more rounds; minutes rather than seconds.
#[test]
#[ignore = "exhaustive; run with --ignored"]
fn deeper_bounds() {
    check_all(2, 3, &Topology::Complete, 3, 20);
    check_all(3, 2, &line(), 2, 20);
    check_all(3, 3, &line(), 2, 10);
    check_all(3, 3, &Topology::Complete, 2, 10);
}
