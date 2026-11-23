//! Compares the ACBBA rule sets over many lossless runs, classifying how each run ends
//! instead of passing or failing it.
//!
//! Run with `ACBBA_CASES=20000 cargo test --release --test it acbba::matrix -- --ignored
//! --nocapture`; the default is 500 cases per arm.

use std::collections::BTreeMap;
use std::env;

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;
use task_allocation::acbba::{Agent, Rules};
use task_allocation::sim::{Network, Simulation, Topology};
use task_allocation::{AgentId, Insertion, TaskId, Valuation};

use super::lossless;
use crate::cbba::{MAX_TEAM, SETTLE_TICKS};
use crate::support::{self, Instance, instance_with};

/// A valuation whose gains never tie with another agent's: each gain is scaled by the team
/// size and offset by the agent's rank. The map is monotone, so gains still diminish
/// wherever the wrapped valuation's do.
#[derive(Clone, Debug)]
pub struct TieFree {
    /// The wrapped valuation.
    inner: support::Agent,

    /// What every gain is multiplied by; one leaves gains unchanged.
    scale: u64,

    /// What is added to every positive gain; below `scale`, and unique to the agent.
    offset: u64,
}

impl TieFree {
    /// `gain`, scaled and offset; zero stays zero.
    const fn map(&self, gain: u64) -> u64 {
        if gain == 0 {
            0
        } else {
            gain * self.scale + self.offset
        }
    }
}

impl Valuation<TaskId> for TieFree {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        self.inner
            .marginal_score(bundle, task)
            .map(|gain| self.map(gain))
    }

    fn best_insertion(&self, path: &[TaskId], task: &TaskId) -> Option<Insertion> {
        self.inner
            .best_insertion(path, task)
            .map(|insertion| Insertion {
                gain: self.map(insertion.gain),
                ..insertion
            })
    }
}

/// One configuration under comparison.
#[derive(Clone, Copy, Debug)]
struct Arm {
    /// The decision table.
    rules: Rules,

    /// The re-announcement period, if any.
    period: Option<u64>,

    /// How gains and ties are handled.
    ties: Ties,
}

/// How an arm handles equal bids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ties {
    /// Integer gains as generated; third parties give ties to the smaller winner.
    Rule,

    /// Integer gains as generated; ties are left as the tables print them.
    Printed,

    /// Gains made tie-free, so no two agents bid alike; ties are left as printed.
    Free,
}

/// How a run ended, worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    /// Messages were still being sent at the end. Only checked without re-announcing.
    NeverQuiet,

    /// Bundles or beliefs still changed over the last two re-announcement periods. Only
    /// checked with re-announcing.
    Unstable,

    /// Two agents hold the same task.
    Overlap,

    /// An agent's bundle disagrees with its own entries.
    Inconsistent,

    /// Conflict-free, but an agent with room could beat a task's real holder, or take a
    /// task nobody holds.
    Beatable,

    /// Stable, but some agent believes in a winner or bid other than the real one.
    BeliefGaps,

    /// Every agent knows the real holder and bid of every task.
    Agreed,
}

/// The team for `instance` under `arm`.
fn team(instance: &Instance, arm: Arm) -> Vec<Agent<TieFree>> {
    let size = u64::try_from(instance.agents.len()).expect("small team");

    instance
        .agents
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, inner)| {
            let rank = u64::try_from(id).expect("small team");
            let free = arm.ties == Ties::Free;
            let valuation = TieFree {
                inner,
                scale: if free { size } else { 1 },
                offset: if free { size - 1 - rank } else { 0 },
            };
            let agent = Agent::new(AgentId(id), instance.tasks, instance.capacity, valuation)
                .with_rules(arm.rules);
            let agent = if arm.ties == Ties::Rule {
                agent
            } else {
                agent.with_printed_ties()
            };

            match arm.period {
                Some(period) => agent.announce_every(period),
                None => agent,
            }
        })
        .collect()
}

/// Runs `instance` over `network` under `arm` and classifies the end.
fn outcome(instance: &Instance, network: Network, arm: Arm) -> Outcome {
    let mut simulation = Simulation::new(team(instance, arm), network);
    if simulation.run_until_settled(SETTLE_TICKS).is_some() {
        return Outcome::Agreed;
    }

    let sent = simulation.stats().sent;
    let before = snapshot(&simulation, instance);
    for _ in 0..20 {
        simulation.step();
    }

    if arm.period.is_none() && simulation.stats().sent != sent {
        return Outcome::NeverQuiet;
    }

    if arm.period.is_some() && snapshot(&simulation, instance) != before {
        return Outcome::Unstable;
    }

    classify(&simulation, instance)
}

/// One agent's bundle, and its believed winner and bid for every task.
type Snapshot = (Vec<TaskId>, Vec<(Option<AgentId>, u64)>);

/// Every agent's [`Snapshot`].
fn snapshot(simulation: &Simulation<Agent<TieFree>>, instance: &Instance) -> Vec<Snapshot> {
    simulation
        .agents()
        .iter()
        .map(|agent| {
            let beliefs = (0..instance.tasks)
                .map(|task| {
                    let claim = agent.claim(TaskId(task));
                    (claim.winner, claim.bid)
                })
                .collect();
            (agent.bundle().tasks().copied().collect(), beliefs)
        })
        .collect()
}

/// Classifies the state of a team for `instance` that is quiet, or keeps re-announcing.
fn classify(simulation: &Simulation<Agent<TieFree>>, instance: &Instance) -> Outcome {
    let assignment = simulation.assignment();
    let agents = simulation.agents();
    if !assignment.is_conflict_free() {
        return Outcome::Overlap;
    }

    if !agents.iter().all(Agent::is_consistent) {
        return Outcome::Inconsistent;
    }

    let held = |task: TaskId| {
        assignment.holder(task).map(|holder| {
            let gain = assignment
                .bundle(holder)
                .entries()
                .find(|(held, _)| **held == task)
                .map_or(0, |(_, gain)| gain);
            (holder, gain)
        })
    };
    let tasks = instance.tasks;

    for agent in agents {
        let bundle = agent.bundle();
        if bundle.len() >= instance.capacity {
            continue;
        }

        for task in (0..tasks).map(TaskId).filter(|task| !bundle.contains(task)) {
            let Some(gain) = agent
                .valuation()
                .best_insertion(bundle.path(), &task)
                .map(|insertion| insertion.gain)
                .filter(|&gain| gain > 0)
            else {
                continue;
            };

            let beats = held(task)
                .is_none_or(|(holder, bid)| gain > bid || (gain == bid && agent.id() < holder));
            if beats {
                return Outcome::Beatable;
            }
        }
    }

    let gaps = agents.iter().any(|agent| {
        (0..tasks).map(TaskId).any(|task| {
            let claim = agent.claim(task);
            let truth = held(task);
            claim.winner != truth.map(|(holder, _)| holder)
                || claim.bid != truth.map_or(0, |(_, bid)| bid)
        })
    });

    if gaps {
        Outcome::BeliefGaps
    } else {
        Outcome::Agreed
    }
}

/// Every arm: each table, with and without re-announcing, under each way of handling ties.
fn arms() -> Vec<Arm> {
    let mut arms = Vec::new();
    for rules in [Rules::Gnc2010, Rules::Infotech2011, Rules::Thesis2012] {
        for period in [None, Some(10)] {
            for ties in [Ties::Rule, Ties::Printed, Ties::Free] {
                arms.push(Arm {
                    rules,
                    period,
                    ties,
                });
            }
        }
    }

    arms
}

/// Runs every arm over the same `ACBBA_CASES` lossless cases and prints how they end, split
/// by complete and sparse topologies.
#[test]
#[ignore = "an experiment; run on demand"]
fn table_matrix() {
    let cases: usize = env::var("ACBBA_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(500);
    let strategy = instance_with(MAX_TEAM).prop_flat_map(|instance| {
        let agents = instance.agents.len();
        (proptest::strategy::Just(instance), lossless(agents))
    });
    let mut runner = TestRunner::deterministic();
    let mut tally: BTreeMap<(String, &str, Outcome), usize> = BTreeMap::new();
    let mut first: BTreeMap<(String, &str, Outcome), (Instance, Network)> = BTreeMap::new();

    for _ in 0..cases {
        let (instance, network) = strategy.new_tree(&mut runner).expect("generates").current();
        let shape = match network.topology {
            Topology::Complete => "complete",
            Topology::Links(_) => "sparse",
        };

        for arm in arms() {
            let end = outcome(&instance, network.clone(), arm);
            let key = (format!("{arm:?}"), shape, end);
            *tally.entry(key.clone()).or_default() += 1;
            first
                .entry(key)
                .or_insert_with(|| (instance.clone(), network.clone()));
        }
    }

    for ((arm, shape, end), count) in &tally {
        println!("{arm} {shape:>8} {end:?}: {count}");
    }

    for ((arm, shape, end), (instance, network)) in &first {
        if *end <= Outcome::Inconsistent {
            println!("first {end:?} for {arm} {shape}: {instance:?} {network:?}");
        }
    }
}
