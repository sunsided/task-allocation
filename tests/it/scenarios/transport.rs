//! Cooperative transport, after Kim and Choi (Journal of Korea Robotics Society 10(3),
//! 2015, pp. 133–138): an object needs two carriers at once, who pick it up together and
//! each drop their side off afterwards.
//!
//! The pickup becomes two copies that need each other and start at the same moment, and
//! each drop-off depends on one copy and starts after it is done. The paper reports a
//! single case and no numbers, so this is a pinned scenario rather than a benchmark.
//!
//! The paper's failure mode is one agent taking every copy of the pickup, which it fixes
//! with an extra reward for taking tasks of different types. Timed CCBBA rejects any path
//! whose own tasks break the timing limits between them, and one agent cannot start two
//! tasks at once, so the copies go to different agents without that reward.

use task_allocation::ccbba::{Constraints, Optimism, TimedAgent};
use task_allocation::route::Route;
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, TaskId};

/// The two copies of the pickup.
const PICKUPS: [TaskId; 2] = [TaskId(0), TaskId(1)];

/// The drop-off after each copy of the pickup.
const DROPOFFS: [TaskId; 2] = [TaskId(2), TaskId(3)];

/// A task anyone can do alone.
const SOLO: TaskId = TaskId(4);

/// Where each task is on the line: the pickup at 10, the drop-offs at 30, the solo task at
/// 50.
const SITES: [u64; 5] = [10, 10, 30, 30, 50];

/// How long each task takes.
const DURATIONS: [u64; 5] = [4, 4, 2, 2, 3];

/// Patience for optimistic bids on a pickup copy whose partner has no winner yet.
const OPTIMISM: Optimism = Optimism {
    timeout: 4,
    solo_attempts: 2,
    any_attempts: 4,
};

/// A carrier on a line at unit speed, earning 100 per task minus one per tick until it
/// finishes.
#[derive(Clone, Debug)]
struct Carrier {
    /// Where the carrier starts.
    origin: u64,
}

impl Route for Carrier {
    type Place = u64;

    fn origin(&self) -> u64 {
        self.origin
    }

    fn place(&self, task: TaskId) -> u64 {
        SITES[task.index()]
    }

    fn travel(&self, from: &u64, to: &u64) -> Option<u64> {
        Some(from.abs_diff(*to))
    }

    fn duration(&self, task: TaskId) -> u64 {
        DURATIONS[task.index()]
    }

    fn reward(&self, _task: TaskId, finish: u64) -> Option<u64> {
        100_u64.checked_sub(finish)
    }
}

/// The transport as CCBBA constraints: the two pickup copies need each other and start
/// together, and each drop-off needs its copy and starts once it is done.
fn transport() -> Constraints {
    let pickup = i64::try_from(DURATIONS[0]).expect("short");
    let members = [PICKUPS[0], PICKUPS[1], DROPOFFS[0], DROPOFFS[1]];
    let dependency: [&[i8]; 4] = [&[0, 1, 1, 0], &[1, 0, 0, 1], &[0, 0, 0, 0], &[0, 0, 0, 0]];
    let timing: [&[Option<i64>]; 4] = [
        &[Some(0), Some(0), Some(-pickup), None],
        &[Some(0), Some(0), None, Some(-pickup)],
        &[None, None, Some(0), None],
        &[None, None, None, Some(0)],
    ];

    let mut constraints = Constraints::new(SITES.len());
    constraints
        .add_timed_activity(&members, &dependency, &timing)
        .expect("the transport is a well-formed activity");

    constraints
}

/// Carriers starting at `origins`, on a lossless network.
fn team(origins: &[u64]) -> Simulation<TimedAgent<Carrier>> {
    let constraints = transport();
    let agents = origins
        .iter()
        .enumerate()
        .map(|(id, &origin)| {
            TimedAgent::new(
                AgentId(id),
                origins.len(),
                SITES.len(),
                SITES.len(),
                Carrier { origin },
                constraints.clone(),
                OPTIMISM,
            )
        })
        .collect();

    Simulation::new(agents, Network::default())
}

/// Three carriers settle on two of them picking the object up together, each dropping its
/// side off afterwards, while the work gets done and every timing limit holds.
#[test]
fn two_carriers_lift_together() {
    let mut simulation = team(&[0, 20, 60]);
    simulation.run_until_settled(500).expect("settles");
    let assignment = simulation.assignment();
    let first = &simulation.agents()[0];

    let carriers = PICKUPS.map(|pickup| assignment.holder(pickup).expect("picked up"));
    assert_ne!(carriers[0], carriers[1]);
    assert_eq!(first.start(PICKUPS[0]), first.start(PICKUPS[1]));
    for (pickup, dropoff) in PICKUPS.into_iter().zip(DROPOFFS) {
        assert!(assignment.holder(dropoff).is_some());
        let lifted = first.start(pickup).expect("scheduled") + DURATIONS[pickup.index()];
        assert!(first.start(dropoff).expect("scheduled") >= lifted);
    }

    assert!(assignment.holder(SOLO).is_some());
    assert!(transport().is_satisfied_by(&assignment));
}

/// A lone carrier cannot lift the object, since it cannot start both copies at once: it
/// leaves the transport alone and does the solo task.
#[test]
fn one_carrier_cannot_lift_alone() {
    let mut simulation = team(&[0]);
    simulation.run_until_settled(500).expect("settles");
    let assignment = simulation.assignment();

    for task in PICKUPS.into_iter().chain(DROPOFFS) {
        assert_eq!(assignment.holder(task), None);
    }

    assert_eq!(assignment.holder(SOLO), Some(AgentId(0)));
}
