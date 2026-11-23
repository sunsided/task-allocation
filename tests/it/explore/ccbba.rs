//! CCBBA under every message schedule on tiny teams, within bounds. CCBBA can cycle
//! forever, so the closing run from a state is not required to settle; the runs that do
//! settle must meet every coupled constraint, agree, and stay within the constrained
//! optimum. The runs that do not are counted.

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;
use task_allocation::AgentId;
use task_allocation::ccbba::{Agent, Constraints};
use task_allocation::sim::{Network, Simulation, Topology};

use super::{Bounds, Report, State, explore};
use crate::bundle::gains_replay;
use crate::ccbba::{OPTIMISM, constrained_optimum, constraints, cycling_scenario, stable};
use crate::explore::cbba::instances;
use crate::support::{self, Instance};

/// Rounds the closing run may take to settle; past it, the run counts as not settling.
const CLOSE_LIMIT: u32 = 60;

/// What exploring one instance found.
#[derive(Clone, Copy, Debug, Default)]
struct Tally {
    /// The explorer's own counts.
    report: Report,

    /// States whose closing run settled.
    settled: usize,

    /// States whose closing run did not settle within [`CLOSE_LIMIT`].
    unsettled: usize,

    /// The longest closing run that settled.
    longest: u32,
}

/// `count` constraint sets over `tasks` tasks with at least one coupled pair, the same
/// every run.
fn coupled(tasks: usize, count: usize) -> Vec<Constraints> {
    let strategy = constraints(tasks).prop_filter("coupled", move |constraints| {
        *constraints != Constraints::new(tasks)
    });
    let mut runner = TestRunner::deterministic();

    (0..count)
        .map(|_| strategy.new_tree(&mut runner).expect("generates").current())
        .collect()
}

/// A CCBBA team for `instance` under `constraints`.
fn team(instance: &Instance, constraints: &Constraints) -> Vec<Agent<support::Agent>> {
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
                constraints.clone(),
                OPTIMISM,
            )
        })
        .collect()
}

/// What holds in every state: each bundle fits its capacity and records gains that replay.
fn honest(state: &State<Agent<support::Agent>>, instance: &Instance) -> Result<(), String> {
    for agent in &state.agents {
        if agent.bundle().len() > instance.capacity {
            return Err(format!("{:?} holds too much", agent.id()));
        }

        gains_replay(agent.valuation(), agent.bundle()).map_err(|error| error.to_string())?;
    }

    Ok(())
}

/// Runs lossless rounds from `state`. When the team settles, checks that it meets the
/// constraints, agrees, and stays within `best`, and returns the rounds it took; `None`
/// when it does not settle.
fn close(
    state: &State<Agent<support::Agent>>,
    topology: &Topology,
    constraints: &Constraints,
    best: u64,
) -> Result<Option<u32>, String> {
    let mut closing = state.clone();
    let Some(rounds) = closing.close(topology, CLOSE_LIMIT) else {
        return Ok(None);
    };

    let network = Network {
        topology: topology.clone(),
        ..Network::default()
    };
    let simulation = Simulation::new(closing.agents, network);
    let assignment = stable(&simulation, constraints).map_err(|error| error.to_string())?;
    if assignment.value() > best {
        return Err(format!("beat the constrained optimum from {state:?}"));
    }

    Ok(Some(rounds))
}

/// Explores `instance` under `constraints` on `topology` within `bounds`.
fn check(
    instance: &Instance,
    constraints: &Constraints,
    topology: &Topology,
    bounds: Bounds,
) -> Result<Tally, String> {
    let best = constrained_optimum(instance, constraints);
    let mut tally = Tally::default();
    let start = State::new(team(instance, constraints));
    tally.report = explore(start, topology, bounds, |state| {
        honest(state, instance)?;
        match close(state, topology, constraints, best)? {
            Some(rounds) => {
                tally.settled += 1;
                tally.longest = tally.longest.max(rounds);
            }
            None => tally.unsettled += 1,
        }

        Ok(())
    })?;

    Ok(tally)
}

/// Pairs of agents that hear each other over three coupled tasks, and three agents on a
/// line over three, `count` of each.
fn shapes(count: usize) -> Vec<(Instance, Constraints, Topology)> {
    let line = Topology::Links(vec![(AgentId(0), AgentId(1)), (AgentId(1), AgentId(2))]);
    let pairs = instances(2, 3, count)
        .into_iter()
        .zip(coupled(3, count))
        .map(|(instance, constraints)| (instance, constraints, Topology::Complete));
    let lines = instances(3, 3, count)
        .into_iter()
        .zip(coupled(3, count))
        .map(|(instance, constraints)| (instance, constraints, line.clone()));

    pairs.chain(lines).collect()
}

/// Checks `count` instances of every shape within `bounds`, printing what was explored,
/// and returns how many closing runs settled and how many did not.
fn check_all(bounds: Bounds, count: usize) -> (usize, usize) {
    let mut totals = (0, 0);
    for (instance, constraints, topology) in shapes(count) {
        let tally = check(&instance, &constraints, &topology, bounds)
            .unwrap_or_else(|error| panic!("{bounds:?}: {error}"));
        println!(
            "ccbba {bounds:?} {} agents: {} states, {} revisits, {} settled (up to {} rounds), {} did not",
            instance.agents.len(),
            tally.report.states,
            tally.report.revisits,
            tally.settled,
            tally.longest,
            tally.unsettled
        );
        totals.0 += tally.settled;
        totals.1 += tally.unsettled;
    }

    totals
}

/// From every lossless schedule, closing runs that settle meet the constraints.
#[test]
fn settled_closing_runs_meet_the_constraints() {
    let (settled, _) = check_all(Bounds { acts: 2, losses: 0 }, 1);

    assert!(settled > 0);
}

/// From every schedule that loses one copy, closing runs that settle meet the constraints.
#[test]
fn settled_closing_runs_meet_the_constraints_despite_a_loss() {
    let (settled, _) = check_all(Bounds { acts: 2, losses: 1 }, 1);

    assert!(settled > 0);
}

/// The explorer sees CCBBA's known cycle: from the pinned scenario, some closing runs never
/// settle, so a count of zero elsewhere means something.
#[test]
fn the_pinned_cycle_does_not_settle() {
    let (instance, constraints) = cycling_scenario();
    let tally = check(
        &instance,
        &constraints,
        &Topology::Complete,
        Bounds { acts: 1, losses: 0 },
    )
    .expect("settled runs meet the constraints");

    assert!(tally.unsettled > 0);
}

/// The same with more instances and deeper bounds, lossless; minutes rather than seconds.
/// With a loss as well, three rounds per agent take hours on the three-agent line.
#[test]
#[ignore = "exhaustive; run with --ignored"]
fn deeper_bounds() {
    check_all(Bounds { acts: 3, losses: 0 }, 3);
    check_all(Bounds { acts: 2, losses: 1 }, 3);
}
