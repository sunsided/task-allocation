//! CBBA under every message schedule on tiny teams, within bounds: in every reachable
//! state each bundle stays honest, and from every one of them lossless rounds settle the
//! team on a stable assignment no better than the optimum.

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;
use task_allocation::AgentId;
use task_allocation::cbba::{Agent, Cadence};
use task_allocation::centralized::optimal;
use task_allocation::sim::{Network, Simulation, Topology};

use super::{Bounds, Report, State, explore};
use crate::bundle::gains_replay;
use crate::cbba::stable;
use crate::support::{self, Instance, instance_sized};

/// Rounds the closing run may take to settle.
const CLOSE_LIMIT: u32 = 40;

/// `count` instances with exactly `agents` agents and `tasks` tasks, the same every run.
pub(super) fn instances(agents: usize, tasks: usize, count: usize) -> Vec<Instance> {
    let strategy = instance_sized(agents, tasks).prop_filter("exact size", move |instance| {
        instance.agents.len() == agents && instance.tasks == tasks
    });
    let mut runner = TestRunner::deterministic();

    (0..count)
        .map(|_| strategy.new_tree(&mut runner).expect("generates").current())
        .collect()
}

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

/// What holds in every state, mid negotiation included: each bundle matches the agent's
/// own entries, fits its capacity, and records gains that replay.
fn honest(state: &State<Agent<support::Agent>>, instance: &Instance) -> Result<(), String> {
    for agent in &state.agents {
        if !agent.is_consistent() {
            return Err(format!(
                "{:?}'s bundle disagrees with its tables",
                agent.id()
            ));
        }

        if agent.bundle().len() > instance.capacity {
            return Err(format!("{:?} holds too much", agent.id()));
        }

        gains_replay(agent.valuation(), agent.bundle()).map_err(|error| error.to_string())?;
    }

    Ok(())
}

/// From `state`, lossless rounds settle the team on a stable assignment worth no more than
/// `best`, the optimum; returns the rounds that took.
fn settles(
    state: &State<Agent<support::Agent>>,
    topology: &Topology,
    instance: &Instance,
    best: u64,
) -> Result<u32, String> {
    let mut closing = state.clone();
    let rounds = closing
        .close(topology, CLOSE_LIMIT)
        .ok_or_else(|| format!("did not settle from {state:?}"))?;

    let network = Network {
        topology: topology.clone(),
        ..Network::default()
    };
    let simulation = Simulation::new(closing.agents, network);
    stable(&simulation, instance).map_err(|error| error.to_string())?;
    if simulation.assignment().value() > best {
        return Err("beat the optimum".into());
    }

    Ok(rounds)
}

/// Explores `instance` on `topology` under `cadence` within `bounds`, checking every
/// state; returns the report and the longest closing run.
fn check(
    instance: &Instance,
    topology: &Topology,
    cadence: Cadence,
    bounds: Bounds,
) -> Result<(Report, u32), String> {
    let best = optimal(&instance.agents, instance.tasks, instance.capacity).value();
    let mut longest = 0;
    let report = explore(
        State::new(team(instance, cadence)),
        topology,
        bounds,
        |state| {
            honest(state, instance)?;
            longest = longest.max(settles(state, topology, instance, best)?);
            Ok(())
        },
    )?;

    Ok((report, longest))
}

/// `count` instances each of two agents that hear each other, over three tasks, and of
/// three agents on a line, where the ends only hear each other through the middle, over two.
fn shapes(count: usize) -> Vec<(Instance, Topology)> {
    let line = Topology::Links(vec![(AgentId(0), AgentId(1)), (AgentId(1), AgentId(2))]);
    let pairs = instances(2, 3, count)
        .into_iter()
        .map(|instance| (instance, Topology::Complete));
    let lines = instances(3, 2, count)
        .into_iter()
        .map(|instance| (instance, line.clone()));

    pairs.chain(lines).collect()
}

/// Checks `count` instances of every shape under `cadence` within `bounds`, printing what
/// was explored.
fn check_all(cadence: Cadence, bounds: Bounds, count: usize) {
    for (instance, topology) in shapes(count) {
        let (report, longest) = check(&instance, &topology, cadence, bounds)
            .unwrap_or_else(|error| panic!("{cadence:?}, {bounds:?}: {error}"));
        println!(
            "{cadence:?} {bounds:?} {} agents: {} states, {} revisits, closing runs up to {longest} rounds",
            instance.agents.len(),
            report.states,
            report.revisits
        );
    }
}

/// The explorer's synchronous closing run matches the simulator on a network without
/// delay or losses, so both drive agents through the same seam.
#[test]
fn closing_runs_match_the_simulator() {
    for (instance, topology) in shapes(3) {
        let network = Network {
            topology: topology.clone(),
            ..Network::default()
        };
        let mut simulation = Simulation::new(team(&instance, Cadence::EveryRound), network);
        let mut state = State::new(team(&instance, Cadence::EveryRound));
        for _ in 0..6 {
            simulation.step();
            while !state.in_flight.is_empty() {
                state.deliver(0);
            }
            for agent in 0..state.agents.len() {
                state.act(agent, &topology);
            }
        }

        assert_eq!(
            format!("{:?}", simulation.agents()),
            format!("{:?}", state.agents)
        );
    }
}

/// Agents that only broadcast after a change settle from every schedule without losses.
#[test]
fn on_change_settles_from_every_lossless_schedule() {
    check_all(
        Cadence::OnChange { refresh: None },
        Bounds { acts: 2, losses: 0 },
        1,
    );
}

/// Agents that broadcast every round settle from every schedule that loses one copy.
#[test]
fn every_round_settles_despite_a_loss() {
    check_all(Cadence::EveryRound, Bounds { acts: 2, losses: 1 }, 1);
}

/// Agents that broadcast after a change and refresh every two rounds settle from every
/// schedule that loses one copy.
#[test]
fn refreshing_settles_despite_a_loss() {
    check_all(
        Cadence::OnChange { refresh: Some(2) },
        Bounds { acts: 2, losses: 1 },
        1,
    );
}

/// The same arms with more instances and deeper bounds; minutes rather than seconds.
#[test]
#[ignore = "exhaustive; run with --ignored"]
fn deeper_bounds() {
    let deep = Bounds { acts: 3, losses: 1 };

    check_all(
        Cadence::OnChange { refresh: None },
        Bounds { acts: 3, losses: 0 },
        3,
    );
    check_all(Cadence::OnChange { refresh: Some(2) }, deep, 3);
    check_all(Cadence::EveryRound, deep, 3);
}

/// The explorer finds what it should: agents that never refresh can lose the one message
/// that mattered and stall, as [`Cadence::OnChange`] documents.
#[test]
fn a_lost_message_stalls_agents_that_never_refresh() {
    let stalls = shapes(3).iter().any(|(instance, topology)| {
        check(
            instance,
            topology,
            Cadence::OnChange { refresh: None },
            Bounds { acts: 2, losses: 1 },
        )
        .is_err()
    });

    assert!(stalls);
}
