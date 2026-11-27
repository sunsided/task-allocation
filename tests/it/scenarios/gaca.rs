//! Open multi-agent routes, scored as a percentage of the optimum, after the benchmark of
//! GACA (Rodriguez, Koenig et al., arXiv 2608.15884, 2026, Sec. V): agents and tasks stand
//! uniformly in a square whose side is drawn from 25 to 55 units, every agent drives from
//! its start through its tasks without returning, and the team minimizes the total
//! distance. Percent optimality is the optimal distance over the protocol's, times 100.
//!
//! The paper solves its instances with a MILP; here the optimum is
//! [`optimal`](task_allocation::centralized::optimal), so instances stay at up to three
//! agents and eight tasks. A reward far above any detour makes every task worth taking, so
//! the most valuable assignment is the shortest one. Its routes do not diminish, so CBBA
//! needs [bid warping](task_allocation::cbba::warp) to settle reliably; the report shows how
//! plain CBBA fares without it.

use std::ops::RangeInclusive;

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use task_allocation::cbba::{Agent, Replan};
use task_allocation::centralized::{optimal, sequential_greedy};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, Assignment, BestInsertion, PathScore, TaskId};

/// Distances are in tenths of a unit, rounded, so that scores stay integers.
const SCALE: f64 = 10.0;

/// The reward per task; above twice the longest distance in the largest square, so every
/// insertion gains.
const REWARD: u64 = 2_000;

/// The most agents in an instance.
const MAX_AGENTS: usize = 3;

/// The most tasks in an instance, to keep the optimum fast.
const MAX_TASKS: usize = 8;

/// Ticks within which a team must settle.
const LIMIT: u64 = 2_000;

/// A point in the square, in units.
type Point = (f64, f64);

/// The distance from `a` to `b`, in tenths of a unit, rounded.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a distance in the square is positive and far below u64::MAX"
)]
fn distance(a: Point, b: Point) -> u64 {
    ((a.0 - b.0).hypot(a.1 - b.1) * SCALE).round() as u64
}

/// An agent's open route: [`REWARD`] per task, minus the distance from its start through
/// its tasks.
#[derive(Clone, Debug)]
struct Route {
    /// Where the agent starts.
    start: Point,

    /// Every task's location, indexed by task.
    sites: Vec<Point>,
}

impl Route {
    /// The length of `path` from the start, in tenths of a unit.
    fn length(&self, path: &[TaskId]) -> u64 {
        let (length, _) = path.iter().fold((0, self.start), |(length, at), task| {
            let site = self.sites[task.index()];
            (length + distance(at, site), site)
        });

        length
    }
}

impl PathScore<TaskId> for Route {
    fn path_score(&self, path: &[TaskId]) -> Option<u64> {
        let earned = REWARD * u64::try_from(path.len()).expect("short path");

        earned.checked_sub(self.length(path))
    }
}

/// Agents and tasks in one square.
#[derive(Clone, Debug)]
struct Instance {
    /// Each agent's route valuation.
    agents: Vec<BestInsertion<Route>>,

    /// The number of tasks.
    tasks: usize,
}

impl Instance {
    /// The total distance of `assignment`'s routes.
    fn distance(&self, assignment: &Assignment) -> u64 {
        self.agents
            .iter()
            .enumerate()
            .map(|(agent, route)| route.0.length(assignment.bundle(AgentId(agent)).path()))
            .sum()
    }

    /// Whether `assignment` gives every task to some agent.
    fn covers(&self, assignment: &Assignment) -> bool {
        (0..self.tasks).all(|task| assignment.holder(TaskId(task)).is_some())
    }
}

/// `agents` agents and `tasks` tasks, uniform in a square whose side is drawn from 25 to 55
/// units.
fn instance(
    agents: RangeInclusive<usize>,
    tasks: RangeInclusive<usize>,
) -> impl Strategy<Value = Instance> {
    (25.0_f64..55.0, agents, tasks).prop_flat_map(|(side, agents, tasks)| {
        let point = move || (0.0..side, 0.0..side);

        (vec(point(), agents), vec(point(), tasks)).prop_map(|(starts, sites)| Instance {
            tasks: sites.len(),
            agents: starts
                .into_iter()
                .map(|start| {
                    BestInsertion(Route {
                        start,
                        sites: sites.clone(),
                    })
                })
                .collect(),
        })
    })
}

/// The allocators the report compares.
#[derive(Clone, Copy, Debug)]
enum Allocator {
    /// CBBA as published, keeping its bundle.
    Cbba,

    /// CBBA rebuilding its bundle every round.
    CbbaFull,

    /// CBBA with bid warping, keeping its bundle.
    CbbaWarped,

    /// The centralized sequential greedy assignment.
    Greedy,
}

impl Allocator {
    /// Every allocator, in report order.
    const ALL: [Self; 4] = [Self::Cbba, Self::CbbaFull, Self::CbbaWarped, Self::Greedy];

    /// The assignment this allocator reaches on `instance`, over a lossless, fully
    /// connected network as in the paper; `None` when CBBA does not settle.
    fn assign(self, instance: &Instance) -> Option<Assignment> {
        let (replan, warping) = match self {
            Self::Greedy => {
                return Some(sequential_greedy(
                    &instance.agents,
                    instance.tasks,
                    instance.tasks,
                ));
            }
            Self::Cbba => (Replan::Keep, false),
            Self::CbbaFull => (Replan::Full, false),
            Self::CbbaWarped => (Replan::Keep, true),
        };

        let size = instance.agents.len();
        let team = instance
            .agents
            .iter()
            .cloned()
            .enumerate()
            .map(|(id, route)| {
                let agent = Agent::new(AgentId(id), size, instance.tasks, instance.tasks, route)
                    .with_replan(replan);
                if warping {
                    agent.with_bid_warping()
                } else {
                    agent
                }
            })
            .collect();
        let mut simulation = Simulation::new(team, Network::default());
        simulation.run_until_settled(LIMIT)?;

        Some(simulation.assignment())
    }
}

/// The optimal assignment of `instance`.
fn best(instance: &Instance) -> Assignment {
    optimal(&instance.agents, instance.tasks, instance.tasks)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Warped CBBA and sequential greedy cover every task, and never drive less than the
    /// optimum, which itself covers every task.
    #[test]
    fn routes_are_never_shorter_than_optimal(instance in instance(1..=MAX_AGENTS, 1..=6)) {
        let optimum = best(&instance);
        prop_assert!(instance.covers(&optimum));

        for allocator in [Allocator::CbbaWarped, Allocator::Greedy] {
            let assignment = allocator.assign(&instance).expect("settles");
            prop_assert!(assignment.is_conflict_free());
            prop_assert!(instance.covers(&assignment), "{:?}", allocator);
            prop_assert!(instance.distance(&assignment) >= instance.distance(&optimum));
        }
    }
}

/// Prints, per allocator, how often it settled on instances of [`MAX_AGENTS`] agents and
/// [`MAX_TASKS`] tasks, and the median, interquartile range, mean and worst of its percent
/// optimality over the runs that settled. Run with `SCENARIO_CASES=500`; the default is 100.
#[test]
#[ignore = "a report; run on demand"]
fn report() {
    let cases: usize = std::env::var("SCENARIO_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(100);
    let strategy = instance(MAX_AGENTS..=MAX_AGENTS, MAX_TASKS..=MAX_TASKS);
    let mut runner = TestRunner::deterministic();
    let mut percents: Vec<Vec<f64>> = vec![Vec::new(); Allocator::ALL.len()];

    for _ in 0..cases {
        let instance = strategy.new_tree(&mut runner).expect("generates").current();
        let optimum = instance.distance(&best(&instance));
        for (allocator, percents) in Allocator::ALL.iter().zip(&mut percents) {
            if let Some(assignment) = allocator.assign(&instance) {
                let distance = instance.distance(&assignment);
                percents.push(if distance == 0 {
                    100.0
                } else {
                    100.0 * ratio(optimum, distance)
                });
            }
        }
    }

    for (allocator, percents) in Allocator::ALL.iter().zip(&mut percents) {
        percents.sort_by(f64::total_cmp);
        let at = |quantile: f64| {
            percents
                .get(index(percents.len(), quantile))
                .copied()
                .unwrap_or(0.0)
        };
        let mean = percents.iter().sum::<f64>()
            / f64::from(u32::try_from(percents.len().max(1)).expect("few runs"));
        println!(
            "{:<10} settled {:>3}/{cases}  percent optimality: median {:>5.1}  IQR {:>5.1}–{:>5.1}  mean {:>5.1}  worst {:>5.1}",
            format!("{allocator:?}"),
            percents.len(),
            at(0.5),
            at(0.25),
            at(0.75),
            mean,
            at(0.0),
        );
    }
}

/// `part / whole`, both distances in tenths of a unit.
fn ratio(part: u64, whole: u64) -> f64 {
    let tenths = |distance: u64| f64::from(u32::try_from(distance).expect("short routes"));

    tenths(part) / tenths(whole)
}

/// The index of `quantile` in a sorted list of `len` values, rounded; zero for an empty one.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a quantile of a short list is a small, positive index"
)]
fn index(len: usize, quantile: f64) -> usize {
    let last = len.saturating_sub(1);

    ((quantile * last as f64).round() as usize).min(last)
}
