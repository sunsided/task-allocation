//! Nailing a timber frame, after the split-delivery routing benchmark of Wang, Zhang and Fang
//! (ISARC 2026, pp. 1570–1577): a robot carries a magazine of nails from a depot to the
//! joints of a frame and back, and the joints at the frame's corners take twice the nails.
//!
//! The frame keeps the paper's shape, shrunk to fit
//! [`optimal`](task_allocation::centralized::optimal): two header joists and a few cross
//! joists, spaced 0.3 to 0.5 m apart and 1 to 2 m long, with a joint where each cross joist
//! meets a header. A joint takes 5 to 11 nails, twice that at a corner. The depot lies on
//! the edge of a workspace from 4 × 2 to 8 × 6 m that holds the frame. The robot's tours
//! become identical agents: each starts and ends at the depot and carries at most one
//! magazine. Splitting one joint's nails over two tours is left out, so a tour either nails
//! a joint completely or not at all. The paper measures the distance gap to the optimum,
//! as here.

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use task_allocation::cbba::Agent;
use task_allocation::centralized::{optimal, sequential_greedy};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, Assignment, BestInsertion, PathScore, TaskId};

/// The robot's tours, each one agent.
const TOURS: usize = 3;

/// The reward per joint; far above any detour in centimeters, so every joint that fits a
/// magazine is worth nailing.
const REWARD: u64 = 10_000;

/// Ticks within which a team must settle.
const LIMIT: u64 = 2_000;

/// A point, in centimeters.
type Point = (i64, i64);

/// The distance from `a` to `b`, in centimeters, rounded.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "distances in the workspace are short and positive"
)]
fn distance(a: Point, b: Point) -> u64 {
    ((a.0 - b.0) as f64).hypot((a.1 - b.1) as f64).round() as u64
}

/// One tour: from the depot through its joints and back, carrying one magazine.
#[derive(Clone, Debug)]
struct Tour {
    /// Where the magazine is refilled.
    depot: Point,

    /// Every joint, indexed by task.
    joints: Vec<Point>,

    /// The nails every joint takes, indexed by task.
    demands: Vec<u64>,

    /// The nails a magazine holds.
    capacity: u64,
}

impl Tour {
    /// The length of the closed tour through `path`, in centimeters.
    fn length(&self, path: &[TaskId]) -> u64 {
        let mut at = self.depot;
        let mut length = 0;
        for task in path {
            let joint = self.joints[task.index()];
            length += distance(at, joint);
            at = joint;
        }

        length + distance(at, self.depot)
    }

    /// The nails `path` takes.
    fn demand(&self, path: &[TaskId]) -> u64 {
        path.iter().map(|task| self.demands[task.index()]).sum()
    }
}

impl PathScore<TaskId> for Tour {
    /// [`REWARD`] per joint minus the closed tour's length; `None` beyond one magazine.
    fn path_score(&self, path: &[TaskId]) -> Option<u64> {
        if self.demand(path) > self.capacity {
            return None;
        }

        let earned = REWARD * u64::try_from(path.len()).expect("short tour");

        earned.checked_sub(self.length(path))
    }
}

/// A frame to nail, with every tour's valuation.
#[derive(Clone, Debug)]
struct Instance {
    /// The tours, identical but for their identifier.
    tours: Vec<BestInsertion<Tour>>,

    /// The number of joints.
    joints: usize,
}

impl Instance {
    /// The total length of `assignment`'s tours that nail anything.
    fn distance(&self, assignment: &Assignment) -> u64 {
        assignment
            .bundles()
            .iter()
            .filter(|bundle| !bundle.is_empty())
            .map(|bundle| self.tours[0].0.length(bundle.path()))
            .sum()
    }

    /// The joints `assignment` nails.
    fn nailed(&self, assignment: &Assignment) -> usize {
        (0..self.joints)
            .filter(|&joint| assignment.holder(TaskId(joint)).is_some())
            .count()
    }
}

/// A frame with `joists` cross joists and magazines of `capacity` nails.
fn instance(joists: usize, capacity: u64) -> impl Strategy<Value = Instance> {
    let spacings = vec(30_i64..=50, joists - 1);
    let demands = vec(5_u64..=11, 2 * joists);
    let workspace = (400_i64..=800, 200_i64..=600);

    (spacings, 100_i64..=200, demands, workspace, 0.0_f64..1.0).prop_map(
        move |(spacings, height, demands, (width, depth), along)| {
            let xs: Vec<i64> = std::iter::once(0)
                .chain(spacings.iter().scan(0, |x, spacing| {
                    *x += spacing;
                    Some(*x)
                }))
                .collect();
            let offset = (50, 50);
            let positions: Vec<Point> = [0, height]
                .into_iter()
                .flat_map(|y| xs.iter().map(move |&x| (offset.0 + x, offset.1 + y)))
                .collect();
            let corner = |joint: usize| joint % joists == 0 || joint % joists == joists - 1;
            let demands = demands
                .into_iter()
                .enumerate()
                .map(|(joint, demand)| if corner(joint) { 2 * demand } else { demand })
                .collect();
            let tour = Tour {
                depot: boundary(
                    width.max(offset.0 + xs[joists - 1] + 50),
                    depth.max(offset.1 + height + 50),
                    along,
                ),
                joints: positions,
                demands,
                capacity,
            };

            Instance {
                joints: 2 * joists,
                tours: vec![BestInsertion(tour); TOURS],
            }
        },
    )
}

/// The point `along` the way around the edge of a `width` × `depth` workspace, from its
/// corner at the origin.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "a workspace edge is a few meters in centimeters"
)]
fn boundary(width: i64, depth: i64, along: f64) -> Point {
    let perimeter = 2 * (width + depth);
    let mut at = (along * perimeter as f64) as i64;
    for (length, corner, step) in [
        (width, (0, 0), (1, 0)),
        (depth, (width, 0), (0, 1)),
        (width, (width, depth), (-1, 0)),
        (depth, (0, depth), (0, -1)),
    ] {
        if at <= length {
            return (corner.0 + step.0 * at, corner.1 + step.1 * at);
        }

        at -= length;
    }

    (0, 0)
}

/// The allocators the report compares.
#[derive(Clone, Copy, Debug)]
enum Allocator {
    /// CBBA as published.
    Cbba,

    /// CBBA with bid warping: tour lengths do not diminish.
    CbbaWarped,

    /// The centralized sequential greedy assignment.
    Greedy,
}

impl Allocator {
    /// Every allocator, in report order.
    const ALL: [Self; 3] = [Self::Cbba, Self::CbbaWarped, Self::Greedy];

    /// The assignment this allocator reaches; `None` when CBBA does not settle.
    fn assign(self, instance: &Instance) -> Option<Assignment> {
        let warping = match self {
            Self::Greedy => {
                return Some(sequential_greedy(
                    &instance.tours,
                    instance.joints,
                    instance.joints,
                ));
            }
            Self::Cbba => false,
            Self::CbbaWarped => true,
        };

        let team = instance
            .tours
            .iter()
            .cloned()
            .enumerate()
            .map(|(id, tour)| {
                let agent = Agent::new(AgentId(id), TOURS, instance.joints, instance.joints, tour);
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
    optimal(&instance.tours, instance.joints, instance.joints)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Warped CBBA and sequential greedy never overfill a magazine, nail no more joints
    /// than the optimum, and drive no less when they nail as many.
    #[test]
    fn magazines_are_never_overfilled(
        instance in (30_u64..=100).prop_flat_map(|capacity| instance(3, capacity)),
    ) {
        let optimum = best(&instance);
        let tour = &instance.tours[0].0;
        for allocator in [Allocator::CbbaWarped, Allocator::Greedy] {
            let assignment = allocator.assign(&instance).expect("settles");
            prop_assert!(assignment.is_conflict_free());
            for bundle in assignment.bundles() {
                prop_assert!(tour.demand(bundle.path()) <= tour.capacity, "{:?}", allocator);
            }

            prop_assert!(instance.nailed(&assignment) <= instance.nailed(&optimum));
            if instance.nailed(&assignment) == instance.nailed(&optimum) {
                prop_assert!(instance.distance(&assignment) >= instance.distance(&optimum));
            }
        }
    }
}

/// Prints, for frames of four cross joists (eight joints) and magazines from 30 to 100
/// nails, how often each allocator settled, how many joints it nailed against the
/// optimum, and its mean distance gap where it nailed as many. Run with
/// `SCENARIO_CASES=50`; the default is 10.
#[test]
#[ignore = "a report; run on demand"]
fn report() {
    let cases: usize = std::env::var("SCENARIO_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(10);

    for capacity in [30, 50, 70, 100] {
        let strategy = instance(4, capacity);
        let mut runner = TestRunner::deterministic();
        let mut rows = [(0_usize, 0_usize, 0_usize, 0.0_f64, 0_usize); Allocator::ALL.len()];
        for _ in 0..cases {
            let instance = strategy.new_tree(&mut runner).expect("generates").current();
            let optimum = best(&instance);
            let (nailed, length) = (instance.nailed(&optimum), instance.distance(&optimum));
            for (allocator, row) in Allocator::ALL.iter().zip(&mut rows) {
                let Some(assignment) = allocator.assign(&instance) else {
                    continue;
                };

                row.0 += 1;
                row.1 += instance.nailed(&assignment);
                row.2 += nailed;
                if instance.nailed(&assignment) == nailed && length > 0 {
                    let gap = instance.distance(&assignment) - length;
                    row.3 += 100.0 * ratio(gap, length);
                    row.4 += 1;
                }
            }
        }

        for (allocator, (settled, nailed, possible, gaps, compared)) in
            Allocator::ALL.iter().zip(rows)
        {
            println!(
                "magazine {capacity:>3}  {:<10} settled {settled:>2}/{cases}  nailed {nailed:>3}/{possible:>3}  gap {:>5.1}% over {compared}",
                format!("{allocator:?}"),
                gaps / f64::from(u32::try_from(compared.max(1)).expect("few runs")),
            );
        }
    }
}

/// `part / whole`, both in centimeters.
fn ratio(part: u64, whole: u64) -> f64 {
    let centimeters = |length: u64| f64::from(u32::try_from(length).expect("short tours"));

    centimeters(part) / centimeters(whole)
}
