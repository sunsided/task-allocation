//! Two field robots split six sample sites around a pond. Travel costs come from RRT*
//! (sampling-planning), which plans around the pond in continuous space. The example
//! compares CBBA with the auction baselines and with the exhaustive optimum.
//!
//! Run with `cargo run --example field_rrt`.

use rand_pcg::Pcg64Mcg;
use sampling_planning::rand_core::SeedableRng;
use sampling_planning::trees::{RrtStar, RrtStarConfig};
use sampling_planning::{
    Euclidean, GoalRegion, Problem, StraightLine, UniformBox, Validity, ValidityChecker,
    discretized_motion,
};
use task_allocation::auction::{combinatorial, sequential};
use task_allocation::cbba::Agent;
use task_allocation::centralized::{optimal, sequential_greedy};
use task_allocation::route::{Route, Timed, rewards};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, Assignment, BestInsertion, TaskId};

/// The field's size, in meters.
const FIELD: f64 = 20.0;

/// The pond: a circle the robots must drive around.
const POND: ([f64; 2], f64) = ([10.0, 10.0], 4.0);

/// Where the robots start.
const STARTS: [[f64; 2]; 2] = [[1.0, 1.0], [19.0, 19.0]];

/// The sample sites.
const SITES: [[f64; 2]; 6] = [
    [10.0, 2.0],
    [18.0, 10.0],
    [10.0, 18.0],
    [2.0, 10.0],
    [5.0, 15.0],
    [15.0, 5.0],
];

/// Everything outside the pond is drivable.
struct Dry;

impl ValidityChecker<Euclidean<2>> for Dry {
    fn check_state(&self, state: &[f64; 2]) -> Validity {
        let ([x, y], radius) = POND;
        let wet = (state[0] - x).hypot(state[1] - y) < radius;

        if wet {
            Validity::Invalid
        } else {
            Validity::Valid
        }
    }

    fn check_motion(&self, space: &Euclidean<2>, from: &[f64; 2], to: &[f64; 2]) -> Validity {
        discretized_motion(space, from, to, 0.05, |state| self.check_state(state))
    }
}

/// The length of a short drivable path from `from` to `to`, in decimeters; `None` when
/// RRT* finds none.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "path lengths on a 20 m field are small and non-negative"
)]
fn driving_distance(from: [f64; 2], to: [f64; 2], seed: u64) -> Option<u64> {
    let problem = Problem {
        space: Euclidean::<2>,
        sampler: UniformBox::new([0.0, 0.0], [FIELD, FIELD]),
        checker: Dry,
        steering: StraightLine::new(1.0),
        start: from,
        goal: GoalRegion::new(to, 0.3),
    };
    let gamma = 1.5 * RrtStarConfig::gamma_lower_bound(2, FIELD * FIELD);
    let mut planner = RrtStar::new(problem, RrtStarConfig::new(gamma, 2.0)).ok()?;
    planner.run(3_000, &mut Pcg64Mcg::seed_from_u64(seed));

    let decimeters = planner.best_path()?.cost * 10.0;

    Some(decimeters.round() as u64)
}

/// Driving distances, in decimeters, between the places a robot may need: its start (index
/// 0) and the sites (index 1 onward).
fn distance_table(start: [f64; 2]) -> Vec<Vec<Option<u64>>> {
    let places: Vec<[f64; 2]> = std::iter::once(start).chain(SITES).collect();

    places
        .iter()
        .enumerate()
        .map(|(row, &from)| {
            places
                .iter()
                .enumerate()
                .map(|(column, &to)| {
                    if row == column {
                        Some(0)
                    } else {
                        driving_distance(from, to, (row * 31 + column) as u64)
                    }
                })
                .collect()
        })
        .collect()
}

/// A robot at 1 m/s, with precomputed driving distances.
#[derive(Clone)]
struct Robot {
    /// Driving distances in decimeters; index 0 is the start, `1 + task` a site.
    table: Vec<Vec<Option<u64>>>,
}

impl Route for Robot {
    type Place = usize;

    fn origin(&self) -> usize {
        0
    }

    fn place(&self, task: TaskId) -> usize {
        1 + task.index()
    }

    /// Seconds from one place to another: a decimeter takes a tenth of a second.
    fn travel(&self, from: &usize, to: &usize) -> Option<u64> {
        self.table[*from][*to].map(|decimeters| decimeters.div_ceil(10))
    }

    fn duration(&self, _task: TaskId) -> u64 {
        10
    }

    /// Samples are worth 100, falling to nothing after two minutes in the sun.
    fn reward(&self, _task: TaskId, finish: u64) -> Option<u64> {
        rewards::linear_decay(100, finish, 0, 120)
    }
}

/// Every robot's valuation.
fn robots() -> Vec<BestInsertion<Timed<Robot>>> {
    STARTS
        .iter()
        .map(|&start| {
            BestInsertion(Timed(Robot {
                table: distance_table(start),
            }))
        })
        .collect()
}

/// Runs CBBA among `valuations` over a lossy network and returns the settled assignment.
fn cbba(valuations: Vec<BestInsertion<Timed<Robot>>>) -> Assignment {
    let team = valuations.len();
    let agents = valuations
        .into_iter()
        .enumerate()
        .map(|(id, valuation)| Agent::new(AgentId(id), team, SITES.len(), 4, valuation))
        .collect();
    let network = Network {
        seed: 5,
        max_delay: 3,
        loss_per_mille: 200,
        ..Network::default()
    };
    let mut simulation = Simulation::new(agents, network);
    simulation
        .run_until_settled(1_000)
        .expect("the robots agree");

    simulation.assignment()
}

/// Prints one line per assignment: its name, value and who holds what.
fn report(name: &str, assignment: &Assignment) {
    let bundles: Vec<Vec<usize>> = assignment
        .bundles()
        .iter()
        .map(|bundle| bundle.path().iter().map(|task| task.index()).collect())
        .collect();
    println!(
        "{name:>22}: value {:>3}, paths {bundles:?}",
        assignment.value()
    );
}

fn main() {
    let valuations = robots();
    let tasks = SITES.len();
    let in_order = (0..tasks).map(TaskId);

    let best = optimal(&valuations, tasks, 4);
    report("optimal", &best);
    report(
        "sequential greedy",
        &sequential_greedy(&valuations, tasks, 4),
    );
    report(
        "combinatorial auction",
        &combinatorial(&valuations, tasks, 4, 8),
    );
    report(
        "sequential auction",
        &sequential(&valuations, tasks, 4, in_order),
    );
    let consensus = cbba(valuations.clone());
    report("CBBA", &consensus);

    assert!(consensus.is_conflict_free());
    assert!(consensus.value() <= best.value());
}
