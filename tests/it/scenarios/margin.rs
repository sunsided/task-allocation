//! Near-boundary and large-margin teams, after the scenario classes of AC-DMTA (Zhao et al.,
//! Drones 10, 2026, Sec. 5.3): in a near-boundary team, small differences in what agents
//! know flip who wins a task; in a large-margin team they rarely do.
//!
//! The paper names the classes but gives no generator, so this one is ours: every task has
//! a base value, and each agent's value of it differs from the base by at most
//! [`NEAR`] in a near-boundary team, while in a large-margin team one agent per task values
//! it at least [`MARGIN`] above everyone else. The defaults follow the paper's setup: six
//! agents, eighteen tasks, bundles of four.

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, TaskId, Valuation, acbba, bwtc, cbba};

use super::{Outcome, Tally, measure};
use crate::acbba::lossless;
use crate::cbba::{SETTLE_TICKS, network};

/// The most an agent's value of a task differs from the task's base value in a
/// near-boundary team.
const NEAR: u64 = 2;

/// How far the favorite of a task values it above everyone else in a large-margin team.
const MARGIN: u64 = 20;

/// Agents in a team, as in AC-DMTA.
const AGENTS: usize = 6;

/// Tasks in a team, as in AC-DMTA.
const TASKS: usize = 18;

/// The most tasks an agent holds, as in AC-DMTA.
const CAPACITY: usize = 4;

/// An agent's values of every task, minus one for every task it already holds, so that
/// gains diminish.
#[derive(Clone, Debug)]
struct Values(Vec<u64>);

impl Valuation<TaskId> for Values {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        let held = u64::try_from(bundle.len()).ok()?;

        self.0[task.index()].checked_sub(held)
    }
}

/// Which kind of team to generate.
#[derive(Clone, Copy, Debug)]
enum Class {
    /// Every agent values every task within [`NEAR`] of its base value.
    NearBoundary,

    /// One agent per task values it at least [`MARGIN`] above everyone else.
    LargeMargin,
}

/// A team of `agents` agents over `tasks` tasks, of `class`.
fn team(class: Class, agents: usize, tasks: usize) -> impl Strategy<Value = Vec<Values>> {
    let bases = vec(30_u64..60, tasks);
    let offsets = vec(vec(0..=2 * NEAR, tasks), agents);
    let favorites = vec(0..agents, tasks);

    (bases, offsets, favorites).prop_map(move |(bases, offsets, favorites)| {
        (0..agents)
            .map(|agent| {
                let values = (0..tasks).map(|task| {
                    let near = bases[task] + offsets[agent][task] - NEAR;
                    let favored = matches!(class, Class::LargeMargin) && favorites[task] == agent;
                    if favored {
                        near + MARGIN + 2 * NEAR
                    } else {
                        near
                    }
                });

                Values(values.collect())
            })
            .collect()
    })
}

/// The protocols a report compares.
#[derive(Clone, Copy, Debug)]
enum Protocol {
    /// CBBA, broadcasting every round.
    Cbba,

    /// CBBA, broadcasting after changes and refreshing every five silent rounds.
    CbbaOnChange,

    /// BW-TC, for networks that lose nothing.
    Bwtc,

    /// ACBBA with the 2011 table, re-announcing every ten ticks.
    Acbba,
}

impl Protocol {
    /// Every protocol, in report order.
    const ALL: [Self; 4] = [Self::Cbba, Self::CbbaOnChange, Self::Bwtc, Self::Acbba];

    /// Runs `values` under this protocol on `network`.
    fn run(self, values: &[Values], network: Network) -> Outcome {
        let agents = values.len();
        let limit = network
            .partitions
            .iter()
            .map(|partition| partition.until)
            .max()
            .unwrap_or(0)
            + SETTLE_TICKS;
        let each = |id: usize| (AgentId(id), values[id].clone());

        match self {
            Self::Cbba | Self::CbbaOnChange => {
                let cadence = match self {
                    Self::Cbba => cbba::Cadence::EveryRound,
                    _ => cbba::Cadence::OnChange { refresh: Some(5) },
                };
                let team = (0..agents)
                    .map(each)
                    .map(|(id, values)| {
                        cbba::Agent::new(id, agents, values.0.len(), CAPACITY, values)
                            .with_cadence(cadence)
                    })
                    .collect();

                measure(
                    &mut Simulation::new(team, network),
                    values[0].0.len(),
                    limit,
                )
            }
            Self::Bwtc => {
                let team = (0..agents)
                    .map(each)
                    .map(|(id, values)| {
                        let tasks = values.0.len();
                        bwtc::Agent::new(id, agents, tasks, CAPACITY, values)
                    })
                    .collect();

                measure(
                    &mut Simulation::new(team, network),
                    values[0].0.len(),
                    limit,
                )
            }
            Self::Acbba => {
                let team = (0..agents)
                    .map(each)
                    .map(|(id, values)| {
                        let tasks = values.0.len();
                        acbba::Agent::new(id, tasks, CAPACITY, values).announce_every(10)
                    })
                    .collect();

                measure(
                    &mut Simulation::new(team, network),
                    values[0].0.len(),
                    limit,
                )
            }
        }
    }
}

/// A team of `class` with two to six agents over up to eighteen tasks, on a lossless
/// network.
fn small(class: Class) -> impl Strategy<Value = (Vec<Values>, Network)> {
    (2..=AGENTS, 1..=TASKS)
        .prop_flat_map(move |(agents, tasks)| (team(class, agents, tasks), lossless(agents)))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Without losses, CBBA, event-driven CBBA and BW-TC settle near-boundary and
    /// large-margin teams on conflict-free assignments.
    #[test]
    fn lossless_teams_settle(
        (values, network) in prop_oneof![small(Class::NearBoundary), small(Class::LargeMargin)],
    ) {
        for protocol in [Protocol::Cbba, Protocol::CbbaOnChange, Protocol::Bwtc] {
            let outcome = protocol.run(&values, network.clone());
            prop_assert!(outcome.settled, "{:?} did not settle", protocol);
        }
    }
}

/// Prints, per class and network, how each protocol fares: settled share, ticks, bytes per
/// assigned task and the share of useful copies. BW-TC runs on lossless networks only.
/// Run with `SCENARIO_CASES=200`; the default is 50.
#[test]
#[ignore = "a report; run on demand"]
fn report() {
    let cases: usize = std::env::var("SCENARIO_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(50);

    for class in [Class::NearBoundary, Class::LargeMargin] {
        for lossy in [false, true] {
            let strategy = team(class, AGENTS, TASKS).prop_flat_map(move |values| {
                let network = if lossy {
                    network(AGENTS).boxed()
                } else {
                    lossless(AGENTS).boxed()
                };
                (Just(values), network)
            });
            let mut tallies = [Tally::default(); Protocol::ALL.len()];
            let mut runner = TestRunner::deterministic();
            for _ in 0..cases {
                let (values, network) =
                    strategy.new_tree(&mut runner).expect("generates").current();
                for (protocol, tally) in Protocol::ALL.iter().zip(&mut tallies) {
                    if lossy && matches!(protocol, Protocol::Bwtc) {
                        continue;
                    }

                    tally.add(protocol.run(&values, network.clone()));
                }
            }

            let network = if lossy { "lossy" } else { "lossless" };
            for (protocol, tally) in Protocol::ALL.iter().zip(&tallies) {
                println!(
                    "{:<12} {network:<8} {:<12} {tally}",
                    format!("{class:?}"),
                    format!("{protocol:?}")
                );
            }
        }
    }
}
