//! Precedence chains, after the benchmark of CTAS-AMP (International Journal of
//! Aeronautical and Space Sciences 27, 2026, pp. 3685–3690): agents and tasks stand
//! uniformly in a 1000 × 1000 m area, agents fly at 10 m/s, every task takes 2 to 10
//! seconds, and runs of consecutive tasks form chains in which each task starts only after
//! its predecessor finished.
//!
//! Each link of a chain becomes a timed activity of [`ccbba`](task_allocation::ccbba): the
//! successor depends on its predecessor and starts at least the predecessor's duration
//! after it. A task's reward falls by one per second until it finishes, so the score
//! charges travel, waiting and execution time as the paper's cost does; its base reward,
//! drawn from 5,000 to 15,000, keeps every task worth taking, as the paper's `α = 100`
//! does. The paper compares against a MILP and BW-CCBBA; the report here measures timed
//! CCBBA alone: how often it settles with every chain respected, and at what cost.

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use task_allocation::ccbba::{Constraints, Optimism, TimedAgent};
use task_allocation::route::Route;
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, TaskId};

use super::{Outcome, Tally, measure};
use crate::acbba::lossless;

/// The side of the square area, in meters.
const AREA: u64 = 1_000;

/// The agents' speed, in meters per second.
const SPEED: u64 = 10;

/// Ticks within which a team must settle.
const LIMIT: u64 = 3_000;

/// Patience for optimistic bids on tasks whose predecessor has no winner yet.
const OPTIMISM: Optimism = Optimism {
    timeout: 4,
    solo_attempts: 2,
    any_attempts: 4,
};

/// A task: where, how long, and its base reward.
#[derive(Clone, Copy, Debug)]
struct Site {
    /// Where the task is, in meters.
    at: (u64, u64),

    /// How long it takes, in seconds.
    duration: u64,

    /// What it is worth if finished at time zero.
    reward: u64,
}

/// An agent flying straight between sites at [`SPEED`], earning each task's reward minus
/// one per second until it finishes.
#[derive(Clone, Debug)]
struct Flight {
    /// Where the agent starts.
    origin: (u64, u64),

    /// The tasks, indexed by task.
    sites: Vec<Site>,
}

impl Route for Flight {
    type Place = (u64, u64);

    fn origin(&self) -> (u64, u64) {
        self.origin
    }

    fn place(&self, task: TaskId) -> (u64, u64) {
        self.sites[task.index()].at
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "coordinates in the area are small and positive"
    )]
    fn travel(&self, from: &(u64, u64), to: &(u64, u64)) -> Option<u64> {
        let meters = (from.0 as f64 - to.0 as f64).hypot(from.1 as f64 - to.1 as f64);

        Some((meters / SPEED as f64).ceil() as u64)
    }

    fn duration(&self, task: TaskId) -> u64 {
        self.sites[task.index()].duration
    }

    fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
        self.sites[task.index()].reward.checked_sub(finish)
    }
}

/// Agents, tasks and chains in one area.
#[derive(Clone, Debug)]
struct Instance {
    /// Each agent's flight.
    agents: Vec<Flight>,

    /// The chains, each a run of consecutive tasks, as `(first task, length)`.
    chains: Vec<(usize, usize)>,

    /// The most tasks an agent holds: 1.3 times the tasks per agent, rounded up, as in the
    /// paper.
    capacity: usize,
}

impl Instance {
    /// The number of tasks.
    fn tasks(&self) -> usize {
        self.agents[0].sites.len()
    }

    /// The chains as CCBBA constraints: in each, every task depends on its predecessor and
    /// starts at least the predecessor's duration after it.
    fn constraints(&self) -> Constraints {
        let mut constraints = Constraints::new(self.tasks());
        let sites = &self.agents[0].sites;
        for &(first, length) in &self.chains {
            let members: Vec<TaskId> = (first..first + length).map(TaskId).collect();
            let mut dependency = vec![vec![0_i8; length]; length];
            let mut timing = vec![vec![None; length]; length];
            for link in 0..length {
                timing[link][link] = Some(0);
                if link + 1 < length {
                    let duration = i64::try_from(sites[first + link].duration).expect("short");
                    dependency[link][link + 1] = 1;
                    timing[link][link + 1] = Some(-duration);
                }
            }

            let dependency: Vec<&[i8]> = dependency.iter().map(Vec::as_slice).collect();
            let timing: Vec<&[Option<i64>]> = timing.iter().map(Vec::as_slice).collect();
            constraints
                .add_timed_activity(&members, &dependency, &timing)
                .expect("a chain is a well-formed activity");
        }

        constraints
    }

    /// Runs timed CCBBA on `network` and sums up the run; it succeeds when the team settles
    /// with every chain respected.
    fn run(&self, network: Network) -> (Outcome, bool) {
        let constraints = self.constraints();
        let team = self.agents.len();
        let agents = self
            .agents
            .iter()
            .cloned()
            .enumerate()
            .map(|(id, flight)| {
                TimedAgent::new(
                    AgentId(id),
                    team,
                    self.tasks(),
                    self.capacity,
                    flight,
                    constraints.clone(),
                    OPTIMISM,
                )
            })
            .collect();
        let mut simulation = Simulation::new(agents, network);
        let outcome = measure(&mut simulation, self.tasks(), LIMIT);
        let respected = outcome.settled && self.respected(&simulation, &constraints);

        (outcome, respected)
    }

    /// Whether the team's assignment meets every dependency, and every agreed pair of
    /// starts every timing limit.
    fn respected(
        &self,
        simulation: &Simulation<TimedAgent<Flight>>,
        constraints: &Constraints,
    ) -> bool {
        let first = &simulation.agents()[0];
        let tasks = (0..self.tasks()).map(TaskId);

        constraints.is_satisfied_by(&simulation.assignment())
            && tasks.clone().all(|task| {
                tasks
                    .clone()
                    .all(|other| match (first.start(task), first.start(other)) {
                        (Some(start), Some(other_start)) => {
                            constraints.starts_agree(task, start, other, other_start)
                        }
                        _ => true,
                    })
            })
    }
}

/// `agents` agents and `tasks` tasks with `chains` chains of `length` each, laid out from
/// task 0 on.
fn instance(
    agents: usize,
    tasks: usize,
    chains: usize,
    length: usize,
) -> impl Strategy<Value = Instance> {
    let point = || (0..=AREA, 0..=AREA);
    let sites = vec((point(), 2_u64..=10, 5_000_u64..=15_000), tasks);

    (vec(point(), agents), sites).prop_map(move |(origins, sites)| {
        let sites: Vec<Site> = sites
            .into_iter()
            .map(|(at, duration, reward)| Site {
                at,
                duration,
                reward,
            })
            .collect();

        Instance {
            agents: origins
                .into_iter()
                .map(|origin| Flight {
                    origin,
                    sites: sites.clone(),
                })
                .collect(),
            chains: (0..chains).map(|chain| (chain * length, length)).collect(),
            capacity: (13 * tasks).div_ceil(10 * agents),
        }
    })
}

/// Two or three agents, with one chain of three among up to eight tasks, on a lossless
/// network.
fn small() -> impl Strategy<Value = (Instance, Network)> {
    (2_usize..=3, 3_usize..=8)
        .prop_flat_map(|(agents, tasks)| instance(agents, tasks, 1, 3))
        .prop_flat_map(|instance| {
            let agents = instance.agents.len();
            (Just(instance), lossless(agents))
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Small teams that settle respect every chain.
    #[test]
    fn settled_chains_are_respected((instance, network) in small()) {
        let (outcome, respected) = instance.run(network);
        prop_assert!(!outcome.settled || respected, "a chain was broken");
    }
}

/// Most small teams settle: at least 95% of 500 drawn with a fixed seed. 494 do.
#[test]
fn most_small_teams_settle() {
    let strategy = small();
    let mut runner = TestRunner::deterministic();
    let settled = (0..500)
        .filter(|_| {
            let (instance, network) = strategy.new_tree(&mut runner).expect("generates").current();
            instance.run(network).0.settled
        })
        .count();

    assert!(settled >= 475, "only {settled} of 500 settled");
}

/// Prints, for scaled-down versions of the paper's settings, how often timed CCBBA settles
/// with every chain respected, and its ticks, bytes per assigned task and share of useful
/// copies, on a fully connected lossless network as in the paper. Setting 1: four agents,
/// one chain of three per ten tasks; setting 3: five agents, thirty tasks, two chains of
/// growing length. Run with `SCENARIO_CASES=100`; the default is 20.
#[test]
#[ignore = "a report; run on demand"]
fn report() {
    let cases: usize = std::env::var("SCENARIO_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(20);
    let settings = [
        ("setting 1", 4, 10, 1, 3),
        ("setting 1", 4, 20, 2, 3),
        ("setting 1", 4, 30, 3, 3),
        ("setting 3", 5, 30, 2, 3),
        ("setting 3", 5, 30, 2, 5),
        ("setting 3", 5, 30, 2, 7),
    ];

    for (name, agents, tasks, chains, length) in settings {
        let strategy = instance(agents, tasks, chains, length);
        let mut runner = TestRunner::deterministic();
        let mut tally = Tally::default();
        let mut respected = 0;
        for _ in 0..cases {
            let instance = strategy.new_tree(&mut runner).expect("generates").current();
            let (outcome, kept) = instance.run(Network::default());
            tally.add(outcome);
            respected += usize::from(kept);
        }

        println!(
            "{name}: {agents} agents, {tasks:>2} tasks, {chains} × {length}  respected {respected:>3}/{cases}  {tally}"
        );
    }
}
