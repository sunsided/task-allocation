//! Unit tests for the simulated network.

use alloc::vec;
use alloc::vec::Vec;

use super::{Network, Partition, Simulation, Topology};
use crate::cbba::Agent;
use crate::centralized::sequential_greedy;
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;

/// Values task `t` at `10 + t` for agent 0 and `20 - t` for others, minus 2 for every task
/// already held.
#[derive(Clone, Copy)]
struct Skewed(usize);

impl Valuation<TaskId> for Skewed {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        let task = u64::try_from(task.index()).ok()?;
        let held = u64::try_from(bundle.len()).ok()?;
        let base = if self.0 == 0 { 10 + task } else { 20 - task };

        base.checked_sub(2 * held)
    }
}

/// A team of `agents` agents over `tasks` tasks, each holding up to `capacity`.
fn team(agents: usize, tasks: usize, capacity: usize) -> Vec<Agent<Skewed>> {
    (0..agents)
        .map(|id| Agent::new(AgentId(id), agents, tasks, capacity, Skewed(id)))
        .collect()
}

/// A lossless, instant network settles on the sequential greedy assignment.
#[test]
fn reliable_network_reaches_the_greedy_assignment() {
    let mut simulation = Simulation::new(team(3, 6, 2), Network::default());

    assert!(simulation.run_until_settled(100).is_some());

    let valuations: Vec<Skewed> = (0..3).map(Skewed).collect();
    assert_eq!(
        simulation.assignment(),
        sequential_greedy(&valuations, 6, 2)
    );
}

/// The same seed gives the same run.
#[test]
fn runs_are_reproducible() {
    let network = Network {
        seed: 11,
        max_delay: 4,
        loss_per_mille: 300,
        topology: Topology::Complete,
        partitions: Vec::new(),
    };
    let mut first = Simulation::new(team(3, 6, 2), network.clone());
    let mut second = Simulation::new(team(3, 6, 2), network);

    assert_eq!(
        first.run_until_settled(1000),
        second.run_until_settled(1000)
    );
    assert_eq!(first.stats(), second.stats());
    assert!(first.stats().lost > 0);
}

/// Partitioned agents cannot settle, and settle once the partition heals.
#[test]
fn partitions_block_until_they_heal() {
    let network = Network {
        partitions: vec![Partition {
            from: 0,
            until: 50,
            groups: vec![0, 1, 1],
        }],
        ..Network::default()
    };
    let mut simulation = Simulation::new(team(3, 6, 2), network);

    assert_eq!(simulation.run_until_settled(50), None);
    assert!(simulation.stats().blocked > 0);
    assert!(simulation.run_until_settled(100).is_some());
    assert!(simulation.tick() > 50);
}

/// A partition separates agents only while active, and agents missing from the groups
/// are on their own.
#[test]
fn partition_membership() {
    let partition = Partition {
        from: 5,
        until: 10,
        groups: vec![0, 0],
    };

    assert!(!partition.separates(5, AgentId(0), AgentId(1)));
    assert!(partition.separates(5, AgentId(0), AgentId(2)));
    assert!(!partition.separates(10, AgentId(0), AgentId(2)));
    assert!(!partition.separates(4, AgentId(0), AgentId(2)));
}

/// Agents must be listed in identifier order.
#[test]
#[should_panic(expected = "in order")]
fn agents_must_be_in_order() {
    let mut agents = team(2, 1, 1);
    agents.swap(0, 1);
    let _ = Simulation::new(agents, Network::default());
}

/// A team without agents has nothing to agree on.
#[test]
fn empty_team_is_settled() {
    let simulation: Simulation<Skewed> = Simulation::new(Vec::new(), Network::default());

    assert!(simulation.is_settled());
    assert_eq!(simulation.assignment().agents(), 0);
}

/// Agents on a line hear of each other only through relays, and still settle.
#[test]
fn line_topology_relays() {
    let line = (1..4)
        .map(|agent| (AgentId(agent - 1), AgentId(agent)))
        .collect();
    let network = Network {
        topology: Topology::Links(line),
        ..Network::default()
    };
    let mut simulation = Simulation::new(team(4, 6, 2), network);

    assert!(simulation.run_until_settled(200).is_some());
    assert!(simulation.assignment().is_conflict_free());
    assert!(simulation.stats().blocked > 0);
    assert!(Topology::Links(vec![(AgentId(0), AgentId(1))]).connects(AgentId(1), AgentId(0)));
    assert!(!Topology::Links(vec![]).connects(AgentId(1), AgentId(0)));
}
